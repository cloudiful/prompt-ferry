use std::collections::HashMap;

use anyhow::anyhow;
use http::{HeaderName, HeaderValue};
use rmcp::{
    ClientServiceExt,
    model::{
        CallToolRequestParams, GetPromptRequestParams, ProtocolVersion, ReadResourceRequestParams,
    },
    service::ClientLifecycleMode,
    transport::{
        ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

use crate::db::McpServer;

use super::{super::protocol::client_info, token_selection::SelectedToken};

pub(super) async fn call_once(
    server: &McpServer,
    selected: SelectedToken,
    request: Value,
) -> anyhow::Result<Value> {
    let client = connect_with_selected(server, selected).await?;
    let result = dispatch(client.peer(), request).await;
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
/// legacy `initialize` handshake when the upstream rejects the discover probe.
///
/// The rmcp `Auto` mode only falls back when the peer answers discover with a
/// JSON-RPC `METHOD_NOT_FOUND` error. Servers built on older SDKs (e.g. rmcp
/// <= 2.2.0) reject the `2026-07-28` probe at the HTTP layer with a 400
/// "Unsupported MCP-Protocol-Version" response, which surfaces as a transport
/// error and would otherwise never recover.
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
        Err(auto_err) => {
            tracing::debug!(server_name = %server.name, error = %auto_err, "mcp auto lifecycle connect failed; retrying with legacy initialize");
            match connect(ClientLifecycleMode::Initialize).await {
                Ok(service) => Ok(service),
                Err(fallback_err) => {
                    tracing::debug!(server_name = %server.name, error = %fallback_err, "mcp legacy initialize fallback also failed");
                    Err(auto_err)
                }
            }
        }
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

async fn dispatch(peer: &rmcp::Peer<rmcp::RoleClient>, request: Value) -> anyhow::Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "tools/list" => peer_list_or_empty(peer.list_all_tools().await, "tools")?,
        "resources/list" => peer_list_or_empty(peer.list_all_resources().await, "resources")?,
        "prompts/list" => peer_list_or_empty(peer.list_all_prompts().await, "prompts")?,
        "tools/call" => serde_json::to_value(
            peer.call_tool(tool_call_params(&params, "tools/call missing params.name")?)
                .await?,
        )?,
        "resources/read" => serde_json::to_value(
            peer.read_resource(ReadResourceRequestParams::new(required_param(
                &params,
                "uri",
                "resources/read missing params.uri",
            )?))
            .await?,
        )?,
        "prompts/get" => serde_json::to_value(
            peer.get_prompt(prompt_params(&params, "prompts/get missing params.name")?)
                .await?,
        )?,
        _ => return Ok(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
    };
    Ok(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
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

fn required_param(params: &Value, key: &str, err: &'static str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!(err))
}

fn tool_call_params(params: &Value, err: &'static str) -> anyhow::Result<CallToolRequestParams> {
    let mut call = CallToolRequestParams::new(required_param(params, "name", err)?);
    if let Some(arguments) = params.get("arguments").and_then(Value::as_object) {
        call = call.with_arguments(arguments.clone());
    }
    Ok(call)
}

fn prompt_params(params: &Value, err: &'static str) -> anyhow::Result<GetPromptRequestParams> {
    let mut prompt = GetPromptRequestParams::new(required_param(params, "name", err)?);
    if let Some(arguments) = params.get("arguments").and_then(Value::as_object) {
        prompt = prompt.with_arguments(arguments.clone());
    }
    Ok(prompt)
}
