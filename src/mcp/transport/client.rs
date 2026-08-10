use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use anyhow::anyhow;
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
        ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess,
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
    server: &McpServer,
    selected: SelectedToken,
    request: Value,
) -> anyhow::Result<Value> {
    let client = connect_with_selected(server, selected).await?;
    let result = dispatch(client.peer(), server, request).await;
    let cancel_result = client.cancel().await;
    match (result, cancel_result) {
        (Ok(value), Ok(_)) => Ok(value),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
    }
}

pub(super) async fn connect_with_selected(
    server: &McpServer,
    selected: SelectedToken,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>> {
    match server.transport.as_str() {
        "http" => {
            let config = http_transport_config(server, selected)?;
            connect_with_lifecycle_fallback(server, move |mode| {
                let config = config.clone();
                async move {
                    let transport = StreamableHttpClientTransport::from_config(config);
                    Ok(client_info().serve_with_lifecycle(transport, mode).await?)
                }
            })
            .await
        }
        "stdio" => {
            connect_with_lifecycle_fallback(server, move |mode| {
                let server = server;
                async move {
                    let command = tokio::process::Command::new(
                        server
                            .command
                            .as_deref()
                            .ok_or_else(|| anyhow!("MCP stdio server missing command"))?,
                    )
                    .configure(|cmd| {
                        cmd.args(json_string_vec(&server.args));
                        for (key, value) in server.env_json.as_object().cloned().unwrap_or_default()
                        {
                            if let Some(value) = value.as_str() {
                                cmd.env(key, value);
                            }
                        }
                    });
                    let transport = TokioChildProcess::new(command)?;
                    Ok(client_info().serve_with_lifecycle(transport, mode).await?)
                }
            })
            .await
        }
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
    server: &McpServer,
    connect: F,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>>
where
    F: Fn(ClientLifecycleMode) -> Fut,
    Fut: std::future::Future<
            Output = anyhow::Result<
                rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>,
            >,
        >,
{
    let cache_key = (server.server_id, server.updated_at);
    let cached = cached_lifecycle(cache_key);
    let (first_mode, first_is_legacy) = match cached {
        Some(UpstreamLifecycle::LegacyInitialize) => (ClientLifecycleMode::Initialize, true),
        Some(UpstreamLifecycle::Auto) | None => (auto_lifecycle_mode(), false),
    };
    let first_label = lifecycle_mode_label(&first_mode);
    match connect(first_mode).await {
        Ok(service) => {
            record_lifecycle(
                cache_key,
                if first_is_legacy {
                    UpstreamLifecycle::LegacyInitialize
                } else {
                    UpstreamLifecycle::Auto
                },
            );
            Ok(service)
        }
        Err(first_err) if is_protocol_rejection(&first_err) => {
            tracing::debug!(
                server_name = %server.name,
                lifecycle = %first_label,
                error = %first_err,
                "mcp lifecycle connect rejected as protocol error; retrying with alternate lifecycle"
            );
            let (second_mode, second_is_legacy) = if first_is_legacy {
                (auto_lifecycle_mode(), false)
            } else {
                (ClientLifecycleMode::Initialize, true)
            };
            let second_label = lifecycle_mode_label(&second_mode);
            match connect(second_mode).await {
                Ok(service) => {
                    record_lifecycle(
                        cache_key,
                        if second_is_legacy {
                            UpstreamLifecycle::LegacyInitialize
                        } else {
                            UpstreamLifecycle::Auto
                        },
                    );
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

/// Freshness window for the learned lifecycle. Long enough that the rejected
/// probe is not replayed on every request, short enough that an upstream
/// upgrade is picked up automatically.
const LIFECYCLE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const LIFECYCLE_CACHE_MAX_ENTRIES: usize = 512;

#[derive(Clone, Copy)]
struct LifecycleCacheEntry {
    learned_at: Instant,
    lifecycle: UpstreamLifecycle,
}

/// Keyed by `(server_id, updated_at)`: editing the server config invalidates
/// the learned lifecycle immediately.
type LifecycleCacheKey = (Uuid, DateTime<Utc>);

#[derive(Default)]
struct LifecycleCacheStore {
    entries: HashMap<LifecycleCacheKey, LifecycleCacheEntry>,
}

impl LifecycleCacheStore {
    fn cached_lifecycle(&mut self, key: LifecycleCacheKey) -> Option<UpstreamLifecycle> {
        match self.entries.get(&key) {
            Some(entry) if entry.learned_at.elapsed() < LIFECYCLE_CACHE_TTL => {
                Some(entry.lifecycle)
            }
            Some(_) => {
                self.entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn record_lifecycle(&mut self, key: LifecycleCacheKey, lifecycle: UpstreamLifecycle) {
        self.entries.insert(
            key,
            LifecycleCacheEntry {
                learned_at: Instant::now(),
                lifecycle,
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

fn cached_lifecycle(key: LifecycleCacheKey) -> Option<UpstreamLifecycle> {
    lock_lifecycle_cache().cached_lifecycle(key)
}

fn record_lifecycle(key: LifecycleCacheKey, lifecycle: UpstreamLifecycle) {
    lock_lifecycle_cache().record_lifecycle(key, lifecycle);
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
    if let Some(rmcp::service::ServiceError::McpError(error_data)) =
        err.downcast_ref::<rmcp::service::ServiceError>()
    {
        return is_protocol_rejection_error(error_data);
    }
    if let Some(rmcp::service::ClientInitializeError::JsonRpcError(error_data)) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return is_protocol_rejection_error(error_data);
    }
    if let Some(rmcp::service::ClientInitializeError::NoCompatibleProtocolVersion { .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return true;
    }
    if let Some(rmcp::service::ClientInitializeError::TransportError { error, .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return error
            .error
            .downcast_ref::<rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>>()
            .is_some_and(is_unsupported_version_response);
    }
    false
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
            message.contains("Unsupported MCP-Protocol-Version")
                || message.contains("MCP-Protocol-Version")
        }
        _ => false,
    }
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
    if let Some(token) = selected.value.filter(|token| !token.trim().is_empty()) {
        config = config.auth_header(token);
    }
    let headers = server
        .http_headers_json
        .as_object()
        .cloned()
        .unwrap_or_default();
    if !headers.is_empty() {
        config = config.custom_headers(parse_http_headers(headers)?);
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

    #[test]
    fn lifecycle_cache_roundtrips_and_invalidates_on_server_update() {
        let mut cache = LifecycleCacheStore::default();
        let key = (Uuid::new_v4(), Utc::now());
        let other_key = (key.0, key.1 + chrono::Duration::seconds(1));

        assert_eq!(cache.cached_lifecycle(key), None);
        cache.record_lifecycle(key, UpstreamLifecycle::LegacyInitialize);
        assert_eq!(
            cache.cached_lifecycle(key),
            Some(UpstreamLifecycle::LegacyInitialize)
        );

        // A bumped updated_at (server edit) must invalidate the entry.
        assert_eq!(cache.cached_lifecycle(other_key), None);
        cache.record_lifecycle(other_key, UpstreamLifecycle::Auto);
        assert_eq!(
            cache.cached_lifecycle(key),
            Some(UpstreamLifecycle::LegacyInitialize)
        );
        assert_eq!(
            cache.cached_lifecycle(other_key),
            Some(UpstreamLifecycle::Auto)
        );

        cache.clear_lifecycle(other_key);
        assert_eq!(cache.cached_lifecycle(other_key), None);
        assert_eq!(
            cache.cached_lifecycle(key),
            Some(UpstreamLifecycle::LegacyInitialize)
        );
    }

    #[test]
    fn lifecycle_cache_expires_entries() {
        let mut cache = LifecycleCacheStore::default();
        let key = (Uuid::new_v4(), Utc::now());
        cache.record_lifecycle(key, UpstreamLifecycle::Auto);
        cache.entries.get_mut(&key).unwrap().learned_at =
            Instant::now() - LIFECYCLE_CACHE_TTL - Duration::from_secs(1);
        assert_eq!(cache.cached_lifecycle(key), None);
    }

    #[test]
    fn lifecycle_cache_evicts_oldest_when_over_capacity() {
        let mut cache = LifecycleCacheStore::default();
        for _ in 0..LIFECYCLE_CACHE_MAX_ENTRIES + 16 {
            cache.record_lifecycle((Uuid::new_v4(), Utc::now()), UpstreamLifecycle::Auto);
        }
        assert!(cache.entries.len() <= LIFECYCLE_CACHE_MAX_ENTRIES);
    }
}
