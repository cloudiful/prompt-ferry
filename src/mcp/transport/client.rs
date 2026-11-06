use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::{
    ClientServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ClientJsonRpcMessage, CompleteRequestParams,
        GetPromptRequestParams, GetPromptResponse, ProtocolVersion, ReadResourceRequestParams,
        ReadResourceResponse,
    },
    service::ClientLifecycleMode,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        auth::AuthError,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError,
            StreamableHttpPostResponse,
        },
    },
};
use serde_json::{Value, json};
use sse_stream::{Error as SseError, Sse};
use uuid::Uuid;

use crate::db::McpServer;

use super::{
    super::protocol::{PREFERRED_PROTOCOL_VERSIONS, client_info},
    token_selection::SelectedToken,
    tool_headers,
};
use crate::mcp::oauth::{McpOAuthError, McpOAuthService};

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
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig>> {
    match server.transport.as_str() {
        "http" => {
            // Issue #368 Phase E + #375 Phase F: `StreamableHttpClientTransportConfig`
            // has no proxy field and `from_config` builds rmcp's internal
            // default client (auto system proxy enabled; unified
            // `socks`/`system-proxy` features via Cargo), so the proxy client
            // is injected via `with_client` (see `super::proxy`). Resolution
            // is row proxy → process env → direct; builtin rows ignore their
            // own row proxy (see `builtin::call`). `Ok(None)` keeps the
            // direct `from_config` path (identical when no proxy env is set);
            // `Err` fails closed without silent direct fallback.
            // `stdio`/`builtin_minimax` never reach here, so they stay direct
            // by construction.
            let config = http_transport_config(server, selected)?;
            let upstream_url = server.url.as_deref().unwrap_or_default();
            // Personal OAuth requires the pinned, proxy-free path: a proxy
            // would move DNS away from the validated addresses and could
            // observe the bearer token.
            if server.effective_auth_mode() == crate::db::MCP_AUTH_MODE_OAUTH {
                super::proxy::oauth_proxy_blocked(server.proxy_url.as_deref(), upstream_url)
                    .map_err(|message| anyhow!("{message}"))?;
                let url = reqwest::Url::parse(upstream_url)
                    .map_err(|_| anyhow!("MCP HTTP server URL is not valid"))?;
                let timeout = Duration::from_millis(server.timeout_ms.max(100) as u64);
                let storage = storage.ok_or_else(|| {
                    anyhow!("personal MCP OAuth requires the worker runtime storage")
                })?;
                let service = storage
                    .oauth()
                    .ok_or_else(|| anyhow!("personal MCP OAuth is not configured"))?
                    .clone();
                #[cfg(test)]
                let pinned = if service.allows_test_network() {
                    super::pinning::loopback_client_for_url(&url, timeout).await?
                } else {
                    super::pinning::pinned_client_for_url(&url, timeout).await?
                };
                #[cfg(not(test))]
                let pinned = super::pinning::pinned_client_for_url(&url, timeout).await?;
                let owner_user_id = server
                    .owner_user_id
                    .ok_or_else(|| anyhow!("personal MCP OAuth requires a server owner"))?;
                let (connection_id, config_fingerprint, url) = service
                    .send_context(server, owner_user_id)
                    .await
                    .map_err(|error| anyhow!("{}", error.message()))?;
                let guarded = GuardedOAuthStreamableClient::new(
                    pinned,
                    service,
                    server.server_id,
                    owner_user_id,
                    connection_id,
                    config_fingerprint,
                    url,
                );
                return connect_with_lifecycle_fallback(
                    Some(storage),
                    server,
                    move |mode, protocol_version| {
                        let config = config.clone();
                        let guarded = guarded.clone();
                        async move {
                            let transport =
                                StreamableHttpClientTransport::with_client(guarded, config);
                            Ok(client_info()
                                .with_protocol_version(protocol_version)
                                .serve_with_lifecycle(transport, mode)
                                .await?)
                        }
                    },
                )
                .await;
            }
            // The error text already carries the redacted proxy (userinfo
            // scrubbed in `super::proxy`); never log the raw row or env value
            // here.
            let proxy_client = super::proxy::client_for_mcp_server_with_row(
                server.proxy_url.as_deref(),
                upstream_url,
            )
            .map_err(|message| {
                tracing::warn!(server_name = %server.name, "{message}");
                anyhow!("{message}")
            })?;
            connect_with_lifecycle_fallback(storage, server, move |mode, protocol_version| {
                let config = config.clone();
                let proxy_client = proxy_client.clone();
                async move {
                    let transport = match proxy_client {
                        Some(client) => StreamableHttpClientTransport::with_client(client, config),
                        None => StreamableHttpClientTransport::from_config(config),
                    };
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
/// The probe is explicit so a rejection always stays visible here. Servers
/// built on older SDKs (e.g. rmcp <= 2.2.0) reject the `2026-07-28` probe at
/// the HTTP layer with a 400 "Unsupported MCP-Protocol-Version" response, and
/// other non-standard servers wrap the rejection in arbitrary JSON-RPC errors;
/// both are classified by [`is_protocol_rejection`] and retried once with the
/// legacy lifecycle at the newest version the rejection advertises.
/// Authentication, DNS, timeout and other transient failures are NOT retried
/// as a full handshake here: they are left for the caller's token failover,
/// which does not replay the lifecycle.
///
/// The lifecycle that actually works is cached per server (keyed by
/// `updated_at`) so subsequent requests skip the rejected probe instead of
/// replaying it on every call. The cache self-heals: when a cached lifecycle
/// starts being rejected, the other one is probed and the cache is relearned.
async fn connect_with_lifecycle_fallback<F, Fut>(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
    connect: F,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig>>
where
    F: Fn(ClientLifecycleMode, ProtocolVersion) -> Fut,
    Fut: std::future::Future<
            Output = anyhow::Result<
                rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig>,
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
        } => (
            discover_lifecycle_mode(),
            false,
            ProtocolVersion::V_2025_11_25,
        ),
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
                (
                    discover_lifecycle_mode(),
                    false,
                    ProtocolVersion::V_2025_11_25,
                )
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

fn discover_lifecycle_mode() -> ClientLifecycleMode {
    // The explicit `Discover` mode is used instead of rmcp's `Auto`: `Auto`
    // retries the legacy handshake on its own (with a fixed version and a
    // folded error), which would hide the rejection from `protocol_rejection`,
    // skip `select_fallback_protocol_version`, and cache a modern lifecycle for
    // a peer that only speaks `initialize`.
    ClientLifecycleMode::Discover {
        preferred_versions: PREFERRED_PROTOCOL_VERSIONS.to_vec(),
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
    service: &rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientConfig>,
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
/// support") phrasing, and both the spaced and hyphenated header spellings.
fn message_mentions_unsupported_protocol(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let names_protocol = lower.contains("protocol version") || lower.contains("protocol-version");
    names_protocol
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
        .and_then(find_supported_versions)
        .or_else(|| parse_supported_versions_from_message(&error_data.message))
        .unwrap_or_default()
}

/// Depth-first search for a `supported` protocol-version list, because
/// gateways wrap the real rejection (and its list, or the naming message) in an
/// outer error's `data`.
fn find_supported_versions(value: &Value) -> Option<Vec<ProtocolVersion>> {
    match value {
        Value::Object(map) => map
            .get("supported")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .or_else(|| map.values().find_map(find_supported_versions)),
        Value::Array(items) => items.iter().find_map(find_supported_versions),
        _ => None,
    }
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
    ensure_continuation_supported(
        peer,
        "tools/call",
        params.input_responses.is_some() || params.request_state.is_some(),
    )?;
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
    ensure_continuation_supported(
        peer,
        "resources/read",
        params.input_responses.is_some() || params.request_state.is_some(),
    )?;
    match peer.read_resource_once(params).await? {
        ReadResourceResponse::Complete(result) => Ok(serde_json::to_value(result)?),
        ReadResourceResponse::InputRequired(result) => Ok(serde_json::to_value(result)?),
        _ => Err(anyhow!("unexpected resources/read response variant")),
    }
}

async fn get_prompt(peer: &rmcp::Peer<rmcp::RoleClient>, params: &Value) -> anyhow::Result<Value> {
    let params = parse_params::<GetPromptRequestParams>(params, "prompts/get missing params.name")?;
    ensure_continuation_supported(
        peer,
        "prompts/get",
        params.input_responses.is_some() || params.request_state.is_some(),
    )?;
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

/// MRTR continuations (`inputResponses`/`requestState`) only exist from
/// `2026-07-28`. Forwarding them to an older upstream would either be silently
/// ignored or lose the continuation, so fail explicitly instead of pretending
/// the retry happened.
fn ensure_continuation_supported(
    peer: &rmcp::Peer<rmcp::RoleClient>,
    method: &str,
    has_continuation: bool,
) -> anyhow::Result<()> {
    if !has_continuation {
        return Ok(());
    }
    let version = negotiated_protocol(peer);
    if version >= ProtocolVersion::V_2026_07_28 {
        return Ok(());
    }
    Err(anyhow!(
        "{method} request carries an MRTR continuation (inputResponses/requestState) that the upstream protocol {} cannot express; refusing to drop or fabricate it",
        version.as_str()
    ))
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
/// connection from its own `ClientConfig`: the downstream's
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

/// The OAuth transport client that guards every actual HTTP send.
///
/// The pinned `reqwest` client performs the request, but each `post_message`,
/// `get_stream` and `delete_session` first revalidates the original authorized
/// server/owner/configuration/connection epoch and obtains a token for that one
/// send only. A token is never stored in the transport configuration, so a
/// paused session whose grant was disconnected, replaced or re-owned cannot
/// keep sending with the old credential.
#[derive(Clone)]
pub(super) struct GuardedOAuthStreamableClient<C = reqwest::Client> {
    inner: C,
    service: Arc<McpOAuthService>,
    server_id: Uuid,
    owner_user_id: i64,
    connection_id: Uuid,
    config_fingerprint: String,
    url: String,
}

impl<C> GuardedOAuthStreamableClient<C> {
    pub(super) fn new(
        inner: C,
        service: Arc<McpOAuthService>,
        server_id: Uuid,
        owner_user_id: i64,
        connection_id: Uuid,
        config_fingerprint: String,
        url: String,
    ) -> Self {
        Self {
            inner,
            service,
            server_id,
            owner_user_id,
            connection_id,
            config_fingerprint,
            url,
        }
    }
}

fn guard_error<E>(error: McpOAuthError) -> StreamableHttpError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match error {
        McpOAuthError::AuthorizationRequired
        | McpOAuthError::NotFound
        | McpOAuthError::Forbidden
        | McpOAuthError::Disabled(_)
        | McpOAuthError::Unavailable => StreamableHttpError::Auth(AuthError::AuthorizationRequired),
        McpOAuthError::RefreshInProgress => {
            StreamableHttpError::Auth(AuthError::AuthorizationFailed(
                "a personal MCP OAuth token refresh is already in progress".to_string(),
            ))
        }
        McpOAuthError::InvalidState | McpOAuthError::Upstream(_) => {
            StreamableHttpError::Auth(AuthError::AuthorizationFailed(
                "the personal MCP OAuth token is unavailable for this send".to_string(),
            ))
        }
    }
}

impl<C> GuardedOAuthStreamableClient<C>
where
    C: StreamableHttpClient + Send + Sync,
{
    async fn send_token(&self) -> Result<String, StreamableHttpError<C::Error>> {
        self.service
            .send_token(
                self.server_id,
                self.owner_user_id,
                self.connection_id,
                &self.config_fingerprint,
                &self.url,
            )
            .await
            .map_err(guard_error)
    }
}

impl<C> StreamableHttpClient for GuardedOAuthStreamableClient<C>
where
    C: StreamableHttpClient + Send + Sync,
{
    type Error = C::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let token = self.send_token().await?;
        self.inner
            .post_message(uri, message, session_id, Some(token), custom_headers)
            .await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let token = self.send_token().await?;
        self.inner
            .post_message_with_max_sse_event_size(
                uri,
                message,
                session_id,
                Some(token),
                custom_headers,
                max_sse_event_size,
            )
            .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let token = self.send_token().await?;
        self.inner
            .delete_session(uri, session_id, Some(token), custom_headers)
            .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let token = self.send_token().await?;
        self.inner
            .get_stream(uri, session_id, last_event_id, Some(token), custom_headers)
            .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let token = self.send_token().await?;
        self.inner
            .get_stream_with_max_sse_event_size(
                uri,
                session_id,
                last_event_id,
                Some(token),
                custom_headers,
                max_sse_event_size,
            )
            .await
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
        crate::db::MCP_AUTH_MODE_OAUTH => {
            // The per-send guard supplies the token, so no token is ever
            // written into the long-lived transport configuration.
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
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: json!([]),
            env_json: json!({}),
            bearer_tokens_json: json!([]),
            http_headers_json: json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: false,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: json!([]),
            disabled_tools: json!([]),
            disabled_resources: json!([]),
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

#[cfg(test)]
mod oauth_guard_tests {
    use std::sync::{Arc, Mutex};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use futures::stream::BoxStream;
    use http::{HeaderName, HeaderValue};
    use rmcp::model::ClientJsonRpcMessage;
    use rmcp::transport::streamable_http_client::{
        StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
    };
    use sse_stream::{Error as SseError, Sse};
    use uuid::Uuid;

    use super::GuardedOAuthStreamableClient;
    use super::client_info;
    use crate::db::{ConfigRepository, McpServerInput, UserStore};
    use crate::mcp::oauth::McpOAuthService;
    use crate::relay_secrets::RelaySecretManager;
    use crate::standalone_config::StandaloneConfigStore;
    use axum::{
        Router,
        body::Body,
        extract::State,
        http::Request,
        middleware::{self, Next},
        response::Response,
    };
    use rmcp::service::ClientLifecycleMode;
    use rmcp::transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    };
    use rmcp::{
        ClientServiceExt, ServerHandler,
        model::{Implementation, InitializeResult, ServerCapabilities, ServerConfig},
        transport::streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OWNER: i64 = 31;
    const OTHER_OWNER: i64 = 32;
    const MCP_URL: &str = "https://mcp.example.test/mcp";

    type RecordedCall = (&'static str, Option<String>);

    /// Records every request that actually reaches the transport boundary,
    /// together with the authorization token it was handed.
    #[derive(Clone, Default)]
    struct CountingClient {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
    }

    impl CountingClient {
        fn record(&self, method: &'static str, token: Option<String>) {
            self.calls.lock().expect("calls").push((method, token));
        }

        fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().expect("calls").clone()
        }

        fn count(&self) -> usize {
            self.calls.lock().expect("calls").len()
        }
    }

    fn message() -> ClientJsonRpcMessage {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .expect("json-rpc notification")
    }

    impl StreamableHttpClient for CountingClient {
        type Error = std::io::Error;

        async fn post_message(
            &self,
            _uri: Arc<str>,
            _message: ClientJsonRpcMessage,
            _session_id: Option<Arc<str>>,
            auth_header: Option<String>,
            _custom_headers: std::collections::HashMap<HeaderName, HeaderValue>,
        ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
            self.record("post_message", auth_header);
            Ok(StreamableHttpPostResponse::Accepted)
        }

        async fn delete_session(
            &self,
            _uri: Arc<str>,
            _session_id: Arc<str>,
            auth_header: Option<String>,
            _custom_headers: std::collections::HashMap<HeaderName, HeaderValue>,
        ) -> Result<(), StreamableHttpError<Self::Error>> {
            self.record("delete_session", auth_header);
            Ok(())
        }

        async fn get_stream(
            &self,
            _uri: Arc<str>,
            _session_id: Option<Arc<str>>,
            _last_event_id: Option<String>,
            auth_header: Option<String>,
            _custom_headers: std::collections::HashMap<HeaderName, HeaderValue>,
        ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>>
        {
            self.record("get_stream", auth_header);
            let stream: BoxStream<'static, Result<Sse, SseError>> =
                Box::pin(futures::stream::empty());
            Ok(stream)
        }
    }

    struct Fixture {
        store: Arc<StandaloneConfigStore>,
        path: std::path::PathBuf,
        repository: ConfigRepository,
        service: Arc<McpOAuthService>,
        server: crate::db::McpServer,
        connection_id: Uuid,
        fingerprint: String,
    }

    impl Fixture {
        async fn new() -> Self {
            Self::new_with_url(MCP_URL).await
        }

        async fn new_with_url(url: &str) -> Self {
            let base = std::env::var("PROMPT_FERRY_TEST_TMPDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir());
            let _ = std::fs::create_dir_all(&base);
            let path = base.join(format!(
                "prompt-ferry-oauth-guard-{}.sqlite",
                Uuid::new_v4()
            ));
            let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
            for user_id in [OWNER, OTHER_OWNER] {
                sqlx::query(
                    "INSERT INTO standalone_users \
                     (user_id, login_name, display_name, password_hash, is_admin, enabled) \
                     VALUES (?1, ?2, ?2, 'hash', 0, 1)",
                )
                .bind(user_id)
                .bind(format!("guard-user-{user_id}"))
                .execute(store.pool())
                .await
                .expect("insert user");
            }
            let manager =
                RelaySecretManager::from_base64(&STANDARD.encode([13_u8; 32])).expect("manager");
            let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
            let server_id = Uuid::new_v4();
            repository
                .create_mcp_server(server_id, server_input("guard server", url))
                .await
                .expect("create server");
            let server = repository
                .get_mcp_server(server_id)
                .await
                .expect("load server")
                .expect("server row");
            let installed = repository
                .install_mcp_oauth_grant(&manager, server_id, OWNER, &grant())
                .await
                .expect("install grant");
            let service = Arc::new(
                McpOAuthService::new(
                    repository.clone(),
                    Some(manager),
                    Some("https://app.example.test".to_string()),
                    UserStore::sqlite(store.pool().clone()),
                )
                .with_private_network(),
            );
            Self {
                store,
                path,
                repository,
                service,
                server,
                connection_id: installed.connection_id,
                fingerprint: installed.config_fingerprint,
            }
        }

        async fn guarded(
            &self,
            client: CountingClient,
        ) -> GuardedOAuthStreamableClient<CountingClient> {
            let (connection_id, fingerprint, url) = self
                .service
                .send_context(&self.server, OWNER)
                .await
                .expect("send context");
            assert_eq!(connection_id, self.connection_id);
            assert_eq!(fingerprint, self.fingerprint);
            GuardedOAuthStreamableClient::new(
                client,
                self.service.clone(),
                self.server.server_id,
                OWNER,
                connection_id,
                fingerprint,
                url,
            )
        }

        async fn guarded_reqwest(
            &self,
            client: reqwest::Client,
        ) -> GuardedOAuthStreamableClient<reqwest::Client> {
            let (connection_id, fingerprint, url) = self
                .service
                .send_context(&self.server, OWNER)
                .await
                .expect("send context");
            GuardedOAuthStreamableClient::new(
                client,
                self.service.clone(),
                self.server.server_id,
                OWNER,
                connection_id,
                fingerprint,
                url,
            )
        }

        async fn close(self) {
            let pool = self.store.pool().clone();
            drop(self.store);
            pool.close().await;
            let _ = std::fs::remove_file(self.path);
        }
    }

    fn server_input(name: &str, url: &str) -> McpServerInput {
        McpServerInput {
            scope: "user".to_string(),
            owner_user_id: Some(OWNER),
            source_endpoint_id: None,
            name: name.to_string(),
            aggregate_naming_mode: "qualified_only".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some(url.to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            auth_mode: "oauth".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
        }
    }

    fn grant() -> crate::db::McpOAuthGrant {
        crate::db::McpOAuthGrant {
            registration: crate::db::McpOAuthRegistration {
                snapshot: serde_json::json!({
                    "client_id": "guard-client",
                    "client_secret": "guard-client-secret",
                    "client_auth_method": "client_secret_basic",
                    "redirect_uri": "https://app.example.test/api/v1/admin/mcp-servers/x/oauth/callback",
                    "resource": MCP_URL,
                    "issuer": "https://issuer.example.test",
                    "metadata": {
                        "authorization_endpoint": "https://issuer.example.test/authorize",
                        "token_endpoint": "https://issuer.example.test/token"
                    }
                }),
            },
            credentials: crate::db::McpOAuthStoredCredentials {
                engine: serde_json::json!({
                    "client_id": "guard-client",
                    "token_response": {
                        "access_token": "guard-token-1",
                        "refresh_token": "guard-refresh-1",
                        "token_type": "Bearer",
                        "expires_in": 3600
                    },
                    "granted_scopes": ["read-mcp"],
                    "token_received_at": 1_760_000_001_i64,
                    "issuer": "https://issuer.example.test"
                }),
                scopes: vec!["read-mcp".to_string()],
                expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
                issuer: Some("https://issuer.example.test".to_string()),
            },
        }
    }

    #[tokio::test]
    async fn every_transport_method_forwards_a_token_for_that_send_only() {
        let fixture = Fixture::new().await;
        let inner = CountingClient::default();
        let guarded = fixture.guarded(inner.clone()).await;

        guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("initialization POST");
        // A follow-up POST on the same paused session is guarded again: the
        // token is not baked into the transport configuration.
        guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("follow-up POST");
        let _stream = guarded
            .get_stream(
                Arc::from("https://mcp.example.test/mcp"),
                None,
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("GET stream");
        guarded
            .delete_session(
                Arc::from("https://mcp.example.test/mcp"),
                Arc::from("session-1"),
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("DELETE session");

        let calls = inner.calls();
        let methods: Vec<_> = calls.iter().map(|(method, _)| *method).collect();
        assert_eq!(
            methods,
            [
                "post_message",
                "post_message",
                "get_stream",
                "delete_session"
            ]
        );
        for (method, token) in calls {
            assert_eq!(
                token.as_deref(),
                Some("guard-token-1"),
                "{method} must carry the guarded token"
            );
        }
        fixture.close().await;
    }

    #[tokio::test]
    async fn a_disconnect_stops_every_further_send() {
        let fixture = Fixture::new().await;
        let inner = CountingClient::default();
        let guarded = fixture.guarded(inner.clone()).await;
        guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("before disconnect");
        assert_eq!(inner.count(), 1);

        fixture
            .service
            .disconnect(&fixture.server, OWNER)
            .await
            .expect("disconnect");

        assert!(
            guarded
                .post_message(
                    Arc::from("https://mcp.example.test/mcp"),
                    message(),
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err()
        );
        assert!(
            guarded
                .get_stream(
                    Arc::from("https://mcp.example.test/mcp"),
                    None,
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err()
        );
        assert!(
            guarded
                .delete_session(
                    Arc::from("https://mcp.example.test/mcp"),
                    Arc::from("session-1"),
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(inner.count(), 1, "no further send may reach the network");
        fixture.close().await;
    }

    #[tokio::test]
    async fn an_owner_or_url_change_stops_the_paused_client() {
        let fixture = Fixture::new().await;
        let inner = CountingClient::default();
        let guarded = fixture.guarded(inner.clone()).await;
        guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("before owner change");

        sqlx::query("UPDATE standalone_mcp_servers SET owner_user_id = ?1 WHERE server_id = ?2")
            .bind(OTHER_OWNER)
            .bind(fixture.server.server_id.to_string())
            .execute(fixture.store.pool())
            .await
            .expect("change owner");
        assert!(
            guarded
                .post_message(
                    Arc::from("https://mcp.example.test/mcp"),
                    message(),
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err(),
            "an owner change must stop the paused client"
        );
        assert_eq!(inner.count(), 1);

        // A separate fixture proves the URL change on its own.
        let url_fixture = Fixture::new().await;
        let url_inner = CountingClient::default();
        let url_guarded = url_fixture.guarded(url_inner.clone()).await;
        url_guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("before url change");
        sqlx::query("UPDATE standalone_mcp_servers SET url = ?1 WHERE server_id = ?2")
            .bind("https://other.example.test/mcp")
            .bind(url_fixture.server.server_id.to_string())
            .execute(url_fixture.store.pool())
            .await
            .expect("change url");
        assert!(
            url_guarded
                .post_message(
                    Arc::from("https://mcp.example.test/mcp"),
                    message(),
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err(),
            "a URL change must stop the paused client"
        );
        assert_eq!(url_inner.count(), 1);

        fixture.close().await;
        url_fixture.close().await;
    }

    #[tokio::test]
    async fn a_reconnect_epoch_stops_the_old_client() {
        let fixture = Fixture::new().await;
        let inner = CountingClient::default();
        let guarded = fixture.guarded(inner.clone()).await;
        guarded
            .post_message(
                Arc::from("https://mcp.example.test/mcp"),
                message(),
                None,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .expect("before reconnect");

        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([13_u8; 32])).expect("manager");
        let reconnected = fixture
            .repository
            .install_mcp_oauth_grant(&manager, fixture.server.server_id, OWNER, &grant())
            .await
            .expect("reinstall grant");
        assert_ne!(reconnected.connection_id, fixture.connection_id);

        assert!(
            guarded
                .post_message(
                    Arc::from("https://mcp.example.test/mcp"),
                    message(),
                    None,
                    None,
                    std::collections::HashMap::new(),
                )
                .await
                .is_err(),
            "a reconnect installs a new epoch that the old client must not use"
        );
        assert_eq!(inner.count(), 1);
        fixture.close().await;
    }

    #[derive(Clone, Default)]
    struct RequestCounters {
        authenticated: Arc<AtomicUsize>,
    }

    async fn count_authenticated(
        State(counters): State<RequestCounters>,
        request: Request<Body>,
        next: Next,
    ) -> Response {
        if request
            .headers()
            .contains_key(axum::http::header::AUTHORIZATION)
        {
            counters.authenticated.fetch_add(1, Ordering::SeqCst);
        }
        next.run(request).await
    }

    #[derive(Clone, Default)]
    struct GuardTestServer;

    impl ServerHandler for GuardTestServer {
        fn get_info(&self) -> ServerConfig {
            InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(Implementation::new("guard-test", "1.0"))
        }
    }

    async fn spawn_guard_mcp() -> (String, RequestCounters) {
        let counters = RequestCounters::default();
        let service = StreamableHttpService::new(
            || Ok(GuardTestServer),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default().disable_allowed_hosts(),
        );
        let router =
            Router::new()
                .nest_service("/mcp", service)
                .layer(middleware::from_fn_with_state(
                    counters.clone(),
                    count_authenticated,
                ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind guard MCP server");
        let address = listener.local_addr().expect("guard MCP address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{address}/mcp"), counters)
    }

    #[tokio::test]
    async fn an_initialized_real_session_stops_after_revocation() {
        for revocation in ["disconnect", "owner", "url"] {
            let (url, counters) = spawn_guard_mcp().await;
            let fixture = Fixture::new_with_url(&url).await;
            let guarded = fixture.guarded_reqwest(reqwest::Client::new()).await;
            let config = StreamableHttpClientTransportConfig::with_uri(url.clone());
            let transport = StreamableHttpClientTransport::with_client(guarded, config);
            let client = client_info()
                .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
                .await
                .expect("real initialize over the guarded client");
            client.list_tools(None).await.expect("initial tools/list");

            let before = counters.authenticated.load(Ordering::SeqCst);
            assert!(before > 0, "the initialized session must authenticate");

            match revocation {
                "disconnect" => {
                    fixture
                        .service
                        .disconnect(&fixture.server, OWNER)
                        .await
                        .expect("disconnect");
                }
                "owner" => {
                    sqlx::query(
                        "UPDATE standalone_mcp_servers SET owner_user_id = ?1 WHERE server_id = ?2",
                    )
                    .bind(OTHER_OWNER)
                    .bind(fixture.server.server_id.to_string())
                    .execute(fixture.store.pool())
                    .await
                    .expect("change owner");
                }
                "url" => {
                    sqlx::query("UPDATE standalone_mcp_servers SET url = ?1 WHERE server_id = ?2")
                        .bind("http://127.0.0.1:1/mcp")
                        .bind(fixture.server.server_id.to_string())
                        .execute(fixture.store.pool())
                        .await
                        .expect("change url");
                }
                other => panic!("unknown revocation {other}"),
            }

            let result = client.list_tools(None).await;
            assert!(
                result.is_err(),
                "{revocation} must stop the initialized session"
            );
            assert_eq!(
                counters.authenticated.load(Ordering::SeqCst),
                before,
                "{revocation} must not send another authenticated request"
            );
            fixture.close().await;
        }
    }
}
