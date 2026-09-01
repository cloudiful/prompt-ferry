use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use http::{HeaderName, HeaderValue};
use rmcp::{
    ClientServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CompleteRequestParams, GetPromptRequestParams,
        GetPromptResponse, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
    },
    service::ClientLifecycleMode,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::db::McpServer;

use super::{
    super::protocol::{PREFERRED_PROTOCOL_VERSIONS, client_info},
    token_selection::SelectedToken,
    tool_headers,
};

pub(super) async fn call_once(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
    selected: SelectedToken,
    request: Value,
    conversation_id: Option<&str>,
) -> anyhow::Result<Value> {
    if server.transport == "builtin_minimax" {
        let storage = storage.ok_or_else(|| anyhow!("MiniMax MCP requires unified storage"))?;
        return super::super::builtin::call(storage, server, &request, conversation_id).await;
    }
    let client = connect_with_selected(storage, server, selected).await?;
    let result = dispatch(client.peer(), server, request).await;
    let cancel_result = client.cancel().await;
    match (result, cancel_result) {
        (Ok(value), Ok(_)) => Ok(value),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
    }
}

pub(super) async fn connect_with_selected(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
    selected: SelectedToken,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>> {
    match server.transport.as_str() {
        "http" => {
            let config = http_transport_config(server, selected)?;
            connect_with_lifecycle_fallback(storage, server, move |mode, protocol_version| {
                let config = config.clone();
                async move {
                    let transport = StreamableHttpClientTransport::from_config(config);
                    Ok(client_info()
                        .with_protocol_version(protocol_version)
                        .serve_with_lifecycle(transport, mode)
                        .await?)
                }
            })
            .await
        }
        "stdio" => {
            connect_with_lifecycle_fallback(storage, server, move |mode, protocol_version| {
                let server = server;
                async move {
                    let command_name = server
                        .command
                        .as_deref()
                        .ok_or_else(|| anyhow!("MCP stdio server missing command"))?;
                    let mut command = tokio::process::Command::new(command_name);
                    command.args(json_string_vec(&server.args));
                    for (key, value) in server.env_json.as_object().cloned().unwrap_or_default() {
                        command.env(key, resolve_env_value(&value)?);
                    }
                    let transport = TokioChildProcess::new(command).map_err(|error| {
                        anyhow!("failed to start MCP stdio command `{command_name}`: {error}")
                    })?;
                    Ok(client_info()
                        .with_protocol_version(protocol_version)
                        .serve_with_lifecycle(transport, mode)
                        .await?)
                }
            })
            .await
        }
        "builtin_minimax" => Err(anyhow!(
            "MiniMax built-in MCP does not use an upstream MCP connection"
        )),
        other => Err(anyhow!("unsupported MCP transport {other}")),
    }
}

/// Connect with the modern `server/discover` lifecycle, falling back to the
/// legacy `initialize` handshake only when the upstream explicitly rejects the
/// discover probe because it cannot speak the requested protocol versions.
///
/// The rmcp `Auto` mode only falls back when the peer answers discover with a
/// JSON-RPC `METHOD_NOT_FOUND` error. Servers built on older SDKs (e.g. rmcp
/// <= 2.2.0) reject the `2026-07-28` probe at the HTTP layer with a 400
/// "Unsupported MCP-Protocol-Version" response, and other non-standard
/// servers wrap the rejection in arbitrary JSON-RPC errors; both surface as
/// transport/JSON-RPC errors that would otherwise never recover, so they are
/// classified by [`is_protocol_rejection`] and retried once with the legacy
/// lifecycle. Authentication, DNS, timeout and other transient failures are
/// NOT retried as a full handshake here: they are left for the caller's token
/// failover, which does not replay the lifecycle.
///
/// The lifecycle that actually works is cached per server (keyed by
/// `updated_at`) so subsequent requests skip the rejected probe instead of
/// replaying it on every call. The cache self-heals: when a cached lifecycle
/// starts being rejected, the other one is probed and the cache is relearned.
async fn connect_with_lifecycle_fallback<F, Fut>(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
    connect: F,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>>
where
    F: Fn(ClientLifecycleMode, ProtocolVersion) -> Fut,
    Fut: std::future::Future<
            Output = anyhow::Result<
                rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>,
            >,
        >,
{
    let cache_key = (server.server_id, server.updated_at);
    let cached = cached_lifecycle(cache_key);
    let (first_mode, first_is_legacy, first_version) = match lifecycle_preference(server, cached) {
        LifecyclePreference {
            mode: UpstreamLifecycle::LegacyInitialize,
            protocol_version,
        } => (
            ClientLifecycleMode::Initialize,
            true,
            protocol_version.unwrap_or(ProtocolVersion::V_2025_11_25),
        ),
        LifecyclePreference {
            mode: UpstreamLifecycle::Auto,
            ..
        } => (auto_lifecycle_mode(), false, ProtocolVersion::V_2025_11_25),
    };
    let first_label = lifecycle_mode_label(&first_mode);
    match connect(first_mode, first_version).await {
        Ok(service) => {
            remember_lifecycle(storage, server, cache_key, first_is_legacy, &service).await;
            Ok(service)
        }
        Err(first_err) if is_protocol_rejection(&first_err) => {
            tracing::debug!(
                server_name = %server.name,
                lifecycle = %first_label,
                error = %first_err,
                "mcp lifecycle connect rejected as protocol error; retrying with alternate lifecycle"
            );
            let rejection = protocol_rejection(&first_err).unwrap_or_default();
            let fallback_version = select_fallback_protocol_version(server, &rejection);
            let (second_mode, second_is_legacy, second_version) = if first_is_legacy {
                (auto_lifecycle_mode(), false, ProtocolVersion::V_2025_11_25)
            } else {
                (ClientLifecycleMode::Initialize, true, fallback_version)
            };
            let second_label = lifecycle_mode_label(&second_mode);
            match connect(second_mode, second_version).await {
                Ok(service) => {
                    remember_lifecycle(storage, server, cache_key, second_is_legacy, &service)
                        .await;
                    Ok(service)
                }
                Err(second_err) => {
                    clear_lifecycle(cache_key);
                    Err(second_err.context(format!(
                        "mcp upstream rejected the {first_label} lifecycle ({first_err:#}); the {second_label} fallback also failed"
                    )))
                }
            }
        }
        Err(auto_err) => Err(auto_err),
    }
}

fn auto_lifecycle_mode() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: PREFERRED_PROTOCOL_VERSIONS.to_vec(),
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

#[derive(Clone, Debug)]
struct LifecyclePreference {
    mode: UpstreamLifecycle,
    protocol_version: Option<ProtocolVersion>,
}

fn lifecycle_preference(
    server: &McpServer,
    cached: Option<CachedLifecycle>,
) -> LifecyclePreference {
    if server.lifecycle_policy == "legacy_initialize" {
        return LifecyclePreference {
            mode: UpstreamLifecycle::LegacyInitialize,
            protocol_version: parse_protocol_version(
                server.lifecycle_manual_protocol_version.as_deref(),
            )
            .or_else(|| {
                parse_protocol_version(server.lifecycle_learned_protocol_version.as_deref())
            })
            .or(Some(ProtocolVersion::V_2025_11_25)),
        };
    }

    let learned_is_current = server.lifecycle_learned_for_updated_at == Some(server.updated_at);
    if learned_is_current {
        if server.lifecycle_learned_mode.as_deref() == Some("legacy_initialize") {
            return LifecyclePreference {
                mode: UpstreamLifecycle::LegacyInitialize,
                protocol_version: parse_protocol_version(
                    server.lifecycle_learned_protocol_version.as_deref(),
                ),
            };
        }
        if server.lifecycle_learned_mode.as_deref() == Some("modern_discover") {
            return LifecyclePreference {
                mode: UpstreamLifecycle::Auto,
                protocol_version: None,
            };
        }
    }

    match cached {
        Some(CachedLifecycle {
            lifecycle: UpstreamLifecycle::LegacyInitialize,
            protocol_version,
        }) => LifecyclePreference {
            mode: UpstreamLifecycle::LegacyInitialize,
            protocol_version,
        },
        Some(CachedLifecycle {
            lifecycle: UpstreamLifecycle::Auto,
            ..
        })
        | None => LifecyclePreference {
            mode: UpstreamLifecycle::Auto,
            protocol_version: None,
        },
    }
}

fn parse_protocol_version(value: Option<&str>) -> Option<ProtocolVersion> {
    value.and_then(|value| serde_json::from_value(Value::String(value.to_string())).ok())
}

async fn remember_lifecycle(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
    cache_key: LifecycleCacheKey,
    is_legacy: bool,
    service: &rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>,
) {
    let mode = if is_legacy {
        UpstreamLifecycle::LegacyInitialize
    } else {
        UpstreamLifecycle::Auto
    };
    let protocol_version = service
        .peer()
        .peer_info()
        .map(|info| info.protocol_version.clone())
        .unwrap_or(ProtocolVersion::V_2025_11_25);
    record_lifecycle(cache_key, mode, Some(protocol_version.clone()));

    let Some(storage) = storage else {
        return;
    };
    let mode = match mode {
        UpstreamLifecycle::Auto => "modern_discover",
        UpstreamLifecycle::LegacyInitialize => "legacy_initialize",
    };
    if let Err(err) = storage
        .repository()
        .mark_mcp_lifecycle_learned(server, mode, protocol_version.as_str())
        .await
    {
        tracing::warn!(
            server_id = %server.server_id,
            error = %err,
            "failed to persist MCP lifecycle compatibility"
        );
    }
}

fn lifecycle_mode_label(mode: &ClientLifecycleMode) -> &'static str {
    match mode {
        ClientLifecycleMode::Initialize => "legacy initialize",
        ClientLifecycleMode::Auto { .. } | ClientLifecycleMode::Discover { .. } => {
            "modern discover"
        }
        _ => "unknown lifecycle",
    }
}

/// The upstream lifecycle that last succeeded for a server, so subsequent
/// requests skip the rejected `server/discover` probe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UpstreamLifecycle {
    Auto,
    LegacyInitialize,
}

/// A cached lifecycle plus the protocol version the successful connection
/// negotiated, so a later recall can replay exactly what worked instead of
/// guessing at an older version that the upstream may reject.
#[derive(Clone, Debug, PartialEq)]
struct CachedLifecycle {
    lifecycle: UpstreamLifecycle,
    protocol_version: Option<ProtocolVersion>,
}

/// Freshness window for the learned lifecycle. Long enough that the rejected
/// probe is not replayed on every request, short enough that an upstream
/// upgrade is picked up automatically.
const LIFECYCLE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const LIFECYCLE_CACHE_MAX_ENTRIES: usize = 512;

#[derive(Clone)]
struct LifecycleCacheEntry {
    learned_at: Instant,
    lifecycle: UpstreamLifecycle,
    protocol_version: Option<ProtocolVersion>,
}

/// Keyed by `(server_id, updated_at)`: editing the server config invalidates
/// the learned lifecycle immediately.
type LifecycleCacheKey = (Uuid, DateTime<Utc>);

#[derive(Default)]
struct LifecycleCacheStore {
    entries: HashMap<LifecycleCacheKey, LifecycleCacheEntry>,
}

impl LifecycleCacheStore {
    fn cached_lifecycle(&mut self, key: LifecycleCacheKey) -> Option<CachedLifecycle> {
        match self.entries.get(&key) {
            Some(entry) if entry.learned_at.elapsed() < LIFECYCLE_CACHE_TTL => {
                Some(CachedLifecycle {
                    lifecycle: entry.lifecycle,
                    protocol_version: entry.protocol_version.clone(),
                })
            }
            Some(_) => {
                self.entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn record_lifecycle(
        &mut self,
        key: LifecycleCacheKey,
        lifecycle: UpstreamLifecycle,
        protocol_version: Option<ProtocolVersion>,
    ) {
        self.entries.insert(
            key,
            LifecycleCacheEntry {
                learned_at: Instant::now(),
                lifecycle,
                protocol_version,
            },
        );
        if self.entries.len() > LIFECYCLE_CACHE_MAX_ENTRIES {
            // Drop the oldest half; entries are cheap to relearn.
            let mut by_age: Vec<_> = self.entries.drain().collect();
            by_age.sort_by_key(|(_, entry)| entry.learned_at);
            by_age.truncate(LIFECYCLE_CACHE_MAX_ENTRIES / 2);
            self.entries.extend(by_age);
        }
    }

    fn clear_lifecycle(&mut self, key: LifecycleCacheKey) {
        self.entries.remove(&key);
    }
}

static LIFECYCLE_CACHE: LazyLock<Mutex<LifecycleCacheStore>> =
    LazyLock::new(|| Mutex::new(LifecycleCacheStore::default()));

fn cached_lifecycle(key: LifecycleCacheKey) -> Option<CachedLifecycle> {
    lock_lifecycle_cache().cached_lifecycle(key)
}

fn record_lifecycle(
    key: LifecycleCacheKey,
    lifecycle: UpstreamLifecycle,
    protocol_version: Option<ProtocolVersion>,
) {
    lock_lifecycle_cache().record_lifecycle(key, lifecycle, protocol_version);
}

fn clear_lifecycle(key: LifecycleCacheKey) {
    lock_lifecycle_cache().clear_lifecycle(key);
}

fn lock_lifecycle_cache() -> std::sync::MutexGuard<'static, LifecycleCacheStore> {
    LIFECYCLE_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// True when the connect error means "this server cannot do the lifecycle
/// that was attempted": a protocol-version rejection (standard JSON-RPC code,
/// message, nested `data`, or HTTP-layer `Unsupported MCP-Protocol-Version`)
/// or a discover response whose supported-version list has no overlap.
/// Anything else (401/403, DNS, timeout, connection reset, unrelated
/// `-32603`) is left untouched.
fn is_protocol_rejection(err: &anyhow::Error) -> bool {
    protocol_rejection(err).is_some()
}

#[derive(Default)]
struct ProtocolRejection {
    supported_versions: Vec<ProtocolVersion>,
}

fn protocol_rejection(err: &anyhow::Error) -> Option<ProtocolRejection> {
    if let Some(rmcp::service::ServiceError::McpError(error_data)) =
        err.downcast_ref::<rmcp::service::ServiceError>()
    {
        return is_protocol_rejection_error(error_data).then(|| ProtocolRejection {
            supported_versions: supported_versions_from_error_data(error_data),
        });
    }
    if let Some(rmcp::service::ClientInitializeError::JsonRpcError(error_data)) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return is_protocol_rejection_error(error_data).then(|| ProtocolRejection {
            supported_versions: supported_versions_from_error_data(error_data),
        });
    }
    if let Some(rmcp::service::ClientInitializeError::NoCompatibleProtocolVersion { .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return Some(ProtocolRejection::default());
    }
    if let Some(rmcp::service::ClientInitializeError::TransportError { error, .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        let error = error
            .error
            .downcast_ref::<rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>>()?;
        if !is_unsupported_version_response(error) && !is_invalid_session_http_rejection(error) {
            return None;
        }
        return Some(ProtocolRejection {
            supported_versions: supported_versions_from_transport_error(error),
        });
    }
    None
}

fn is_protocol_rejection_error(error_data: &rmcp::ErrorData) -> bool {
    if is_rejection_code(i64::from(error_data.code.0)) {
        return true;
    }
    if message_mentions_unsupported_protocol(&error_data.message) {
        return true;
    }
    error_data
        .data
        .as_ref()
        .is_some_and(json_contains_protocol_rejection)
}

fn is_rejection_code(code: i64) -> bool {
    matches!(
        code,
        -32601 /* METHOD_NOT_FOUND */ | -32022 /* UNSUPPORTED_PROTOCOL_VERSION */
    )
}

/// Message check for rejections carried by non-standard JSON-RPC codes
/// (e.g. `-32000 Bad Request` from a gateway) that still name the protocol.
/// Matches both the participle ("not supported") and base form ("does not
/// support") phrasing.
fn message_mentions_unsupported_protocol(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("protocol version")
        && (lower.contains("unsupported")
            || lower.contains("not supported")
            || lower.contains("does not support"))
}

/// Recursively searches a JSON-RPC `data` payload for a nested
/// unsupported-protocol-version error: some servers/gateways wrap the real
/// rejection inside a generic error (`-32603`/`-32000`) whose `data` carries
/// the original message and supported-version list.
fn json_contains_protocol_rejection(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if let Some(message) = map.get("message").and_then(Value::as_str)
                && message_mentions_unsupported_protocol(message)
            {
                return true;
            }
            if map
                .get("code")
                .and_then(Value::as_i64)
                .is_some_and(is_rejection_code)
            {
                return true;
            }
            map.values().any(json_contains_protocol_rejection)
        }
        Value::Array(items) => items.iter().any(json_contains_protocol_rejection),
        Value::String(text) => message_mentions_unsupported_protocol(text),
        _ => false,
    }
}

fn is_unsupported_version_response(
    error: &rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>,
) -> bool {
    match error {
        rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
            message,
        ) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("unsupported mcp-protocol-version")
                || lower.contains("mcp-protocol-version")
                || message_mentions_unsupported_protocol(message)
        }
        _ => false,
    }
}

/// Narrow classifier for stateful servers (e.g. Grafana mcp-go) that answer
/// the session-less `server/discover` probe with a non-standard HTTP-layer
/// rejection instead of a JSON-RPC method-not-found error: an HTTP 404 whose
/// body says the session id is invalid. Only this exact shape counts as a
/// lifecycle rejection so the legacy initialize fallback runs; arbitrary 404s
/// (e.g. a wrong URL), auth failures, DNS/timeout errors, and unrelated
/// session errors must not trigger the fallback.
fn is_invalid_session_http_rejection(
    error: &rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>,
) -> bool {
    let rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
        message,
    ) = error
    else {
        return false;
    };
    let lower = message.to_ascii_lowercase();
    lower.contains("http 404") && lower.contains("invalid session id")
}

fn supported_versions_from_error_data(error_data: &rmcp::ErrorData) -> Vec<ProtocolVersion> {
    error_data
        .data
        .as_ref()
        .and_then(|data| data.get("supported"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

fn supported_versions_from_transport_error(
    error: &rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>,
) -> Vec<ProtocolVersion> {
    let rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
        message,
    ) = error
    else {
        return Vec::new();
    };
    let Some((_, body)) = message.split_once(": ") else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    value
        .pointer("/error/data/supported")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .or_else(|| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .and_then(parse_supported_versions_from_message)
        })
        .unwrap_or_default()
}

fn parse_supported_versions_from_message(message: &str) -> Option<Vec<ProtocolVersion>> {
    let values = message
        .split_once("supported versions:")?
        .1
        .trim()
        .trim_matches(|character| character == ')' || character == ']');
    let versions = values
        .split(',')
        .filter_map(|value| parse_protocol_version(Some(value.trim())))
        .collect::<Vec<_>>();
    (!versions.is_empty()).then_some(versions)
}

fn select_fallback_protocol_version(
    server: &McpServer,
    rejection: &ProtocolRejection,
) -> ProtocolVersion {
    if server.lifecycle_policy == "legacy_initialize" {
        return parse_protocol_version(server.lifecycle_manual_protocol_version.as_deref())
            .or_else(|| {
                parse_protocol_version(server.lifecycle_learned_protocol_version.as_deref())
            })
            .unwrap_or(ProtocolVersion::V_2025_11_25);
    }
    // The spec does not guarantee the server lists its `supported` versions in
    // any order, so prefer the NEWEST version the server declares that the
    // client can speak, excluding the version that just failed the probe.
    let fallback = rejection
        .supported_versions
        .iter()
        .filter(|version| **version != ProtocolVersion::V_2026_07_28)
        .max_by(|a, b| a.as_str().cmp(b.as_str()))
        .cloned();
    if let Some(version) = fallback {
        return version;
    }
    parse_protocol_version(server.lifecycle_learned_protocol_version.as_deref())
        .unwrap_or(ProtocolVersion::V_2025_11_25)
}

pub(super) fn peer_list_or_empty<T: serde::Serialize>(
    result: Result<Vec<T>, rmcp::ServiceError>,
    result_key: &str,
) -> anyhow::Result<Value> {
    match result {
        Ok(items) => Ok(json!({ result_key: items })),
        Err(err) if err.to_string().contains("Method not found") => Ok(json!({ result_key: [] })),
        Err(err) => Err(err.into()),
    }
}

async fn dispatch(
    peer: &rmcp::Peer<rmcp::RoleClient>,
    server: &McpServer,
    request: Value,
) -> anyhow::Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "tools/list" => peer_list_or_empty(peer.list_all_tools().await, "tools")?,
        "resources/list" => peer_list_or_empty(peer.list_all_resources().await, "resources")?,
        "resources/templates/list" => peer_list_or_empty(
            peer.list_all_resource_templates().await,
            "resourceTemplates",
        )?,
        "prompts/list" => peer_list_or_empty(peer.list_all_prompts().await, "prompts")?,
        "tools/call" => call_tool(peer, server, &params).await?,
        "resources/read" => read_resource(peer, &params).await?,
        "prompts/get" => get_prompt(peer, &params).await?,
        "completion/complete" => complete(peer, &params).await?,
        _ => return Ok(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
    };
    Ok(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

async fn call_tool(
    peer: &rmcp::Peer<rmcp::RoleClient>,
    server: &McpServer,
    params: &Value,
) -> anyhow::Result<Value> {
    let params = parse_params::<CallToolRequestParams>(params, "tools/call missing params")?;
    let version = negotiated_protocol(peer);
    let response =
        tool_headers::call_tool_with_warmup(peer, params, &version, server.transport == "http")
            .await?;
    match response {
        CallToolResponse::Complete(result) => Ok(serde_json::to_value(result)?),
        CallToolResponse::InputRequired(result) => Ok(serde_json::to_value(result)?),
        CallToolResponse::Task(_) => Err(anyhow!(
            "upstream returned a task for tools/call, but the prompt-ferry MCP proxy does not support the tasks extension"
        )),
        _ => Err(anyhow!("unexpected tools/call response variant")),
    }
}

async fn read_resource(
    peer: &rmcp::Peer<rmcp::RoleClient>,
    params: &Value,
) -> anyhow::Result<Value> {
    let params =
        parse_params::<ReadResourceRequestParams>(params, "resources/read missing params.uri")?;
    match peer.read_resource_once(params).await? {
        ReadResourceResponse::Complete(result) => Ok(serde_json::to_value(result)?),
        ReadResourceResponse::InputRequired(result) => Ok(serde_json::to_value(result)?),
        _ => Err(anyhow!("unexpected resources/read response variant")),
    }
}

async fn get_prompt(peer: &rmcp::Peer<rmcp::RoleClient>, params: &Value) -> anyhow::Result<Value> {
    let params = parse_params::<GetPromptRequestParams>(params, "prompts/get missing params.name")?;
    match peer.get_prompt_once(params).await? {
        GetPromptResponse::Complete(result) => Ok(serde_json::to_value(result)?),
        GetPromptResponse::InputRequired(result) => Ok(serde_json::to_value(result)?),
        _ => Err(anyhow!("unexpected prompts/get response variant")),
    }
}

async fn complete(peer: &rmcp::Peer<rmcp::RoleClient>, params: &Value) -> anyhow::Result<Value> {
    let params =
        parse_params::<CompleteRequestParams>(params, "completion/complete missing params")?;
    Ok(serde_json::to_value(peer.complete(params).await?)?)
}

/// The protocol version negotiated with the upstream, defaulting to the
/// legacy version before the handshake completes.
fn negotiated_protocol(peer: &rmcp::Peer<rmcp::RoleClient>) -> ProtocolVersion {
    peer.peer_info()
        .map(|info| info.protocol_version.clone())
        .unwrap_or(ProtocolVersion::V_2025_11_25)
}

/// Full serde deserialization of request params so `_meta`, `inputResponses`,
/// and `requestState` survive the proxy instead of being rebuilt from a few
/// hardcoded fields.
fn parse_params<T>(params: &Value, err: &'static str) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned + rmcp::model::RequestParamsMeta,
{
    let mut params: T = serde_json::from_value(params.clone()).map_err(|error| {
        anyhow!(
            "{err}: invalid params ({error}); use a typed MCP client that sends complete request params"
        )
    })?;
    strip_transport_meta(&mut params);
    Ok(params)
}

/// Transport-level request metadata must be regenerated by the upstream
/// connection from its own `ClientInfo`: the downstream's
/// `protocolVersion`/`clientInfo`/`clientCapabilities`/`logLevel` must not be
/// forced onto an upstream that negotiated a different version. Trace context
/// and progress tokens are preserved.
fn strip_transport_meta<T: rmcp::model::RequestParamsMeta>(params: &mut T) {
    const RESERVED_KEYS: [&str; 4] = [
        "io.modelcontextprotocol/protocolVersion",
        "io.modelcontextprotocol/clientInfo",
        "io.modelcontextprotocol/clientCapabilities",
        "io.modelcontextprotocol/logLevel",
    ];
    let Some(meta) = params.meta_mut() else {
        return;
    };
    for key in RESERVED_KEYS {
        meta.remove(key);
    }
}

fn http_transport_config(
    server: &McpServer,
    selected: SelectedToken,
) -> anyhow::Result<StreamableHttpClientTransportConfig> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(
        server
            .url
            .as_deref()
            .ok_or_else(|| anyhow!("MCP HTTP server missing url"))?
            .to_string(),
    );
    let effective_mode = server.effective_auth_mode();
    let headers = server
        .http_headers_json
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut custom_headers = if headers.is_empty() {
        HashMap::new()
    } else {
        parse_http_headers(headers)?
    };
    match effective_mode {
        crate::db::MCP_AUTH_MODE_BASIC => {
            let username = server.basic_username.as_deref().unwrap_or("").trim();
            let password = server.basic_password.as_deref().unwrap_or("").trim();
            if !username.is_empty() && !password.is_empty() {
                let credentials = format!("{username}:{password}");
                let encoded = STANDARD.encode(credentials);
                let value = format!("Basic {encoded}");
                custom_headers.insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&value)
                        .map_err(|_| anyhow!("invalid basic auth header value"))?,
                );
            }
            if !custom_headers.is_empty() {
                config = config.custom_headers(custom_headers);
            }
        }
        crate::db::MCP_AUTH_MODE_BEARER => {
            if let Some(token) = selected.value.filter(|token| !token.trim().is_empty()) {
                config = config.auth_header(token);
            }
            if !custom_headers.is_empty() {
                config = config.custom_headers(custom_headers);
            }
        }
        _ => {
            if !custom_headers.is_empty() {
                config = config.custom_headers(custom_headers);
            }
        }
    }
    Ok(config)
}

fn parse_http_headers(
    headers: serde_json::Map<String, Value>,
) -> anyhow::Result<HashMap<HeaderName, HeaderValue>> {
    let mut parsed = HashMap::new();
    for (name, value) in headers {
        if let Some(reserved) = crate::db::RESERVED_MCP_HTTP_HEADERS
            .iter()
            .find(|reserved| name.eq_ignore_ascii_case(reserved))
        {
            return Err(anyhow!(
                "http_headers_json must not override reserved header `{reserved}`"
            ));
        }
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("http_headers_json values must be strings"))?;
        parsed.insert(
            HeaderName::try_from(name.as_str())
                .map_err(|_| anyhow!("invalid HTTP header name: {name}"))?,
            HeaderValue::from_str(value)
                .map_err(|_| anyhow!("invalid HTTP header value for {name}"))?,
        );
    }
    Ok(parsed)
}

fn json_string_vec(value: &Value) -> Vec<String> {
    value
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn resolve_env_value(value: &Value) -> anyhow::Result<String> {
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("MCP stdio environment values must be strings"))?;
    let Some(name) = crate::db::mcp_env_reference_name(value) else {
        return Ok(value.to_string());
    };
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        Ok(_) => Err(anyhow!(
            "MCP stdio worker environment variable {name} is empty"
        )),
        Err(error) => Err(anyhow!(
            "MCP stdio worker environment variable {name} is unavailable: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use rmcp::{ErrorData, model::ErrorCode, service::ClientInitializeError};

    use super::*;

    fn error_data(code: i32, message: &str, data: Option<Value>) -> ErrorData {
        ErrorData::new(ErrorCode(code), message.to_string(), data)
    }

    fn json_rpc_error(code: i32, message: &str, data: Option<Value>) -> anyhow::Error {
        anyhow::Error::new(ClientInitializeError::JsonRpcError(error_data(
            code, message, data,
        )))
    }

    fn mcp_error(code: i32, message: &str, data: Option<Value>) -> anyhow::Error {
        anyhow::Error::new(rmcp::service::ServiceError::McpError(error_data(
            code, message, data,
        )))
    }

    #[test]
    fn rejects_standard_protocol_error_codes() {
        assert!(is_protocol_rejection(&json_rpc_error(
            -32022,
            "Unsupported protocol version",
            None
        )));
        assert!(is_protocol_rejection(&json_rpc_error(
            -32601,
            "Method not found",
            None
        )));
        assert!(is_protocol_rejection(&mcp_error(
            -32022,
            "Unsupported protocol version",
            None
        )));
        assert!(is_protocol_rejection(&mcp_error(
            -32601,
            "Method not found",
            None
        )));
    }

    #[test]
    fn rejects_nonstandard_error_with_protocol_message() {
        assert!(is_protocol_rejection(&json_rpc_error(
            -32000,
            "Unsupported protocol version: 2026-07-28",
            None,
        )));
        assert!(is_protocol_rejection(&json_rpc_error(
            -32000,
            "The requested protocol version (2026-07-28) is not supported",
            None,
        )));
        assert!(is_protocol_rejection(&json_rpc_error(
            -32000,
            "The server does not support protocol version 2026-07-28",
            None,
        )));
    }

    #[test]
    fn rejects_nested_error_wrapped_in_generic_internal_error() {
        // The screenshot scenario: a generic -32603/-32000 outer error whose
        // `data` carries the real unsupported-version message and list.
        for outer in [-32603, -32000] {
            let wrapped = json_rpc_error(
                outer,
                "Internal error",
                Some(json!({
                    "error": {
                        "code": -32000,
                        "message": "Bad Request",
                        "data": {
                            "message": "Unsupported protocol version: 2026-07-28",
                            "supported": ["2025-11-25", "2025-06-18"],
                        },
                    }
                })),
            );
            assert!(
                is_protocol_rejection(&wrapped),
                "outer code {outer} must be classified"
            );
        }
    }

    #[test]
    fn rejects_no_compatible_protocol_version() {
        let err = anyhow::Error::new(ClientInitializeError::NoCompatibleProtocolVersion {
            client_supported: vec![ProtocolVersion::V_2026_07_28],
            server_supported: vec![],
        });
        assert!(is_protocol_rejection(&err));
    }

    #[test]
    fn ignores_unrelated_errors() {
        assert!(!is_protocol_rejection(&json_rpc_error(
            -32603,
            "Internal server error",
            Some(json!({ "err": "boom" })),
        )));
        assert!(!is_protocol_rejection(&json_rpc_error(
            -32000,
            "Bad Request",
            Some(json!({ "detail": "invalid json body" })),
        )));
        assert!(!is_protocol_rejection(&json_rpc_error(
            -32600,
            "Invalid Request",
            None
        )));
        assert!(!is_protocol_rejection(&anyhow::Error::new(
            ClientInitializeError::ConnectionClosed("reset".to_string()),
        )));
        assert!(!is_protocol_rejection(&anyhow::Error::new(
            ClientInitializeError::NoPreferredProtocolVersion,
        )));
    }

    fn streamable_http_transport_error(
        inner: rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>,
    ) -> anyhow::Error {
        anyhow::Error::new(ClientInitializeError::TransportError {
            error: rmcp::transport::DynamicTransportError::from_parts(
                "streamable_http_client",
                std::any::TypeId::of::<StreamableHttpClientTransport<reqwest::Client>>(),
                Box::new(inner),
            ),
            context: "connect".into(),
        })
    }

    #[test]
    fn classifies_grafana_shaped_invalid_session_404() {
        let err = streamable_http_transport_error(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 404 Not Found: Invalid session ID".into(),
            ),
        );
        assert!(is_protocol_rejection(&err));
    }

    #[test]
    fn invalid_session_classification_is_case_insensitive() {
        let err = streamable_http_transport_error(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 404: {\"error\":\"invalid session id\"}".into(),
            ),
        );
        assert!(is_protocol_rejection(&err));
    }

    #[test]
    fn does_not_classify_other_404s_or_statuses() {
        // 404 without invalid-session wording (e.g. a wrong URL).
        assert!(!is_invalid_session_http_rejection(
            &rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 404 Not Found: page not found".into(),
            )
        ));
        assert!(!is_protocol_rejection(&streamable_http_transport_error(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 404 Not Found: page not found".into(),
            ),
        )));

        // Invalid-session wording on a non-404 status must not match.
        assert!(!is_invalid_session_http_rejection(
            &rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 400 Bad Request: Invalid session ID".into(),
            )
        ));
        assert!(!is_protocol_rejection(&streamable_http_transport_error(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                "HTTP 400 Bad Request: Invalid session ID".into(),
            ),
        )));

        // Other transport error variants are never this rejection.
        assert!(!is_protocol_rejection(&streamable_http_transport_error(
            rmcp::transport::streamable_http_client::StreamableHttpError::ServerDoesNotSupportSse,
        )));
    }

    #[test]
    fn lifecycle_cache_roundtrips_and_invalidates_on_server_update() {
        let mut cache = LifecycleCacheStore::default();
        let key = (Uuid::new_v4(), Utc::now());
        let other_key = (key.0, key.1 + chrono::Duration::seconds(1));

        assert_eq!(cache.cached_lifecycle(key), None);
        cache.record_lifecycle(
            key,
            UpstreamLifecycle::LegacyInitialize,
            Some(ProtocolVersion::V_2025_06_18),
        );
        assert_eq!(
            cache.cached_lifecycle(key).map(|c| c.lifecycle),
            Some(UpstreamLifecycle::LegacyInitialize)
        );
        assert_eq!(
            cache.cached_lifecycle(key).and_then(|c| c.protocol_version),
            Some(ProtocolVersion::V_2025_06_18)
        );

        // A bumped updated_at (server edit) must invalidate the entry.
        assert_eq!(cache.cached_lifecycle(other_key), None);
        cache.record_lifecycle(other_key, UpstreamLifecycle::Auto, None);
        assert_eq!(
            cache.cached_lifecycle(key).map(|c| c.lifecycle),
            Some(UpstreamLifecycle::LegacyInitialize)
        );
        assert_eq!(
            cache.cached_lifecycle(other_key).map(|c| c.lifecycle),
            Some(UpstreamLifecycle::Auto)
        );

        cache.clear_lifecycle(other_key);
        assert_eq!(cache.cached_lifecycle(other_key), None);
        assert_eq!(
            cache.cached_lifecycle(key).map(|c| c.lifecycle),
            Some(UpstreamLifecycle::LegacyInitialize)
        );
    }

    #[test]
    fn lifecycle_cache_expires_entries() {
        let mut cache = LifecycleCacheStore::default();
        let key = (Uuid::new_v4(), Utc::now());
        cache.record_lifecycle(key, UpstreamLifecycle::Auto, None);
        cache.entries.get_mut(&key).unwrap().learned_at =
            Instant::now() - LIFECYCLE_CACHE_TTL - Duration::from_secs(1);
        assert_eq!(cache.cached_lifecycle(key), None);
    }

    #[test]
    fn lifecycle_cache_evicts_oldest_when_over_capacity() {
        let mut cache = LifecycleCacheStore::default();
        for _ in 0..LIFECYCLE_CACHE_MAX_ENTRIES + 16 {
            cache.record_lifecycle((Uuid::new_v4(), Utc::now()), UpstreamLifecycle::Auto, None);
        }
        assert!(cache.entries.len() <= LIFECYCLE_CACHE_MAX_ENTRIES);
    }

    #[test]
    fn parses_supported_versions_from_grep_style_message() {
        let versions = parse_supported_versions_from_message(
            "Bad Request: Unsupported protocol version (supported versions: 2025-06-18, 2025-03-26, 2024-11-05, 2024-10-07)",
        )
        .expect("versions parsed");
        let rendered: Vec<String> = versions
            .iter()
            .map(|version| version.as_str().to_string())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "2025-06-18".to_string(),
                "2025-03-26".to_string(),
                "2024-11-05".to_string(),
                "2024-10-07".to_string()
            ]
        );
    }

    #[test]
    fn parses_supported_versions_from_transport_http_400_without_content_type() {
        let error = rmcp::transport::streamable_http_client::StreamableHttpError::<reqwest::Error>::UnexpectedServerResponse(
            std::borrow::Cow::Owned(
                "HTTP 400: {\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"Bad Request: Unsupported protocol version (supported versions: 2025-06-18, 2025-03-26, 2024-11-05, 2024-10-07)\"},\"id\":null}".to_string(),
            ),
        );
        assert!(is_unsupported_version_response(&error));
        let versions = supported_versions_from_transport_error(&error);
        assert_eq!(versions.len(), 4);
        assert_eq!(versions[0].as_str(), "2025-06-18");
    }

    #[test]
    fn selects_highest_supported_version_for_legacy_fallback() {
        let server = test_server();
        let rejection = ProtocolRejection {
            supported_versions: vec![
                ProtocolVersion::V_2026_07_28,
                ProtocolVersion::V_2025_11_25,
                ProtocolVersion::V_2025_06_18,
            ],
        };
        let version = select_fallback_protocol_version(&server, &rejection);
        assert_eq!(version, ProtocolVersion::V_2025_11_25);
    }

    #[test]
    fn selects_newest_version_even_when_server_lists_ascending() {
        // The spec does not order `supported`; a server may list versions
        // oldest-first. We must still negotiate the newest compatible one.
        let server = test_server();
        let rejection = ProtocolRejection {
            supported_versions: vec![
                ProtocolVersion::V_2024_11_05,
                ProtocolVersion::V_2025_03_26,
                ProtocolVersion::V_2025_06_18,
            ],
        };
        let version = select_fallback_protocol_version(&server, &rejection);
        assert_eq!(version, ProtocolVersion::V_2025_06_18);
    }

    #[test]
    fn fallback_skips_versions_the_client_cannot_parse() {
        let server = test_server();
        let rejection = ProtocolRejection {
            supported_versions: vec![ProtocolVersion::V_2026_07_28],
        };
        let version = select_fallback_protocol_version(&server, &rejection);
        assert_eq!(version, ProtocolVersion::V_2025_11_25);
    }

    #[test]
    fn manual_legacy_policy_pins_the_protocol_version() {
        let mut server = test_server();
        server.lifecycle_policy = "legacy_initialize".to_string();
        server.lifecycle_manual_protocol_version = Some("2025-03-26".to_string());
        let version = select_fallback_protocol_version(&server, &ProtocolRejection::default());
        assert_eq!(version.as_str(), "2025-03-26");
    }

    #[test]
    fn learned_legacy_mode_is_preferred_for_current_config() {
        let mut server = test_server();
        server.lifecycle_learned_mode = Some("legacy_initialize".to_string());
        server.lifecycle_learned_protocol_version = Some("2025-06-18".to_string());
        server.lifecycle_learned_for_updated_at = Some(server.updated_at);
        let preference = lifecycle_preference(&server, None);
        assert!(matches!(
            preference.mode,
            UpstreamLifecycle::LegacyInitialize
        ));
        assert_eq!(preference.protocol_version.unwrap().as_str(), "2025-06-18");
    }

    #[test]
    fn stale_learned_legacy_mode_falls_back_to_auto_probe() {
        let mut server = test_server();
        server.lifecycle_learned_mode = Some("legacy_initialize".to_string());
        server.lifecycle_learned_protocol_version = Some("2025-06-18".to_string());
        server.lifecycle_learned_for_updated_at =
            Some(server.updated_at - chrono::Duration::days(1));
        let preference = lifecycle_preference(&server, None);
        assert!(matches!(preference.mode, UpstreamLifecycle::Auto));
    }

    #[test]
    fn cached_legacy_recall_replays_the_negotiated_version() {
        let server = test_server();
        let cached = Some(CachedLifecycle {
            lifecycle: UpstreamLifecycle::LegacyInitialize,
            protocol_version: Some(ProtocolVersion::V_2025_06_18),
        });
        let preference = lifecycle_preference(&server, cached);
        assert!(matches!(
            preference.mode,
            UpstreamLifecycle::LegacyInitialize
        ));
        assert_eq!(preference.protocol_version.unwrap().as_str(), "2025-06-18");
    }

    #[test]
    fn resolves_worker_environment_references() {
        assert_eq!(
            resolve_env_value(&json!("plain-value")).unwrap(),
            "plain-value"
        );
        assert_eq!(
            crate::db::mcp_env_reference_name("{env:MINIMAX_API_KEY}"),
            Some("MINIMAX_API_KEY")
        );
        assert_eq!(
            crate::db::mcp_env_reference_name("{env:MINIMAX-API-KEY}"),
            None
        );
    }

    fn test_server() -> McpServer {
        McpServer {
            server_id: Uuid::new_v4(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "test".to_string(),
            aggregate_naming_mode: "qualified_only".to_string(),
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: json!([]),
            env_json: json!({}),
            bearer_tokens_json: json!([]),
            http_headers_json: json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: json!([]),
            disabled_tools: json!([]),
            disabled_resources: json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
            lifecycle_learned_mode: None,
            lifecycle_learned_protocol_version: None,
            lifecycle_learned_for_updated_at: None,
            lifecycle_learned_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}
