use std::collections::HashMap;

use anyhow::anyhow;
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

use crate::db::McpServer;

use super::{super::protocol::client_info, token_selection::SelectedToken, tool_headers};

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
/// "Unsupported MCP-Protocol-Version" response, which surfaces as a transport
/// error and would otherwise never recover. Authentication, DNS, timeout and
/// other transient failures are NOT retried as a full handshake here: they are
/// left for the caller's token failover, which does not replay the lifecycle.
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
    let auto_mode = ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    };
    match connect(auto_mode).await {
        Ok(service) => Ok(service),
        Err(auto_err) if is_protocol_rejection(&auto_err) => {
            tracing::debug!(server_name = %server.name, error = %auto_err, "mcp auto lifecycle connect failed with protocol rejection; retrying with legacy initialize");
            match connect(ClientLifecycleMode::Initialize).await {
                Ok(service) => Ok(service),
                Err(fallback_err) => {
                    tracing::debug!(server_name = %server.name, error = %fallback_err, "mcp legacy initialize fallback also failed");
                    Err(auto_err)
                }
            }
        }
        Err(auto_err) => Err(auto_err),
    }
}

/// True when the connect error means "this server cannot do the modern
/// lifecycle": an HTTP-layer `Unsupported MCP-Protocol-Version` rejection or
/// a JSON-RPC `METHOD_NOT_FOUND`/protocol-version error. Anything else
/// (401/403, DNS, timeout, connection reset) is left untouched.
fn is_protocol_rejection(err: &anyhow::Error) -> bool {
    if let Some(rmcp::service::ServiceError::McpError(rmcp::ErrorData { code, .. })) =
        err.downcast_ref::<rmcp::service::ServiceError>()
    {
        return matches!(
            code.0,
            -32601 /* METHOD_NOT_FOUND */ | -32022 /* UNSUPPORTED_PROTOCOL_VERSION */
        );
    }
    if let Some(rmcp::service::ClientInitializeError::TransportError { error, .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return error
            .error
            .downcast_ref::<rmcp::transport::streamable_http_client::StreamableHttpError<reqwest::Error>>()
            .is_some_and(is_unsupported_version_response);
    }
    if let Some(rmcp::service::ClientInitializeError::JsonRpcError(rmcp::ErrorData {
        code, ..
    })) = err.downcast_ref::<rmcp::service::ClientInitializeError>()
    {
        return matches!(code.0, -32601 | -32022);
    }
    false
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
