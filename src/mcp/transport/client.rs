use std::collections::HashMap;

use anyhow::anyhow;
use http::{HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams},
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
            let transport = StreamableHttpClientTransport::from_config(http_transport_config(
                server, selected,
            )?);
            Ok(client_info().serve(transport).await?)
        }
        "stdio" => {
            let transport = TokioChildProcess::new(
                tokio::process::Command::new(
                    server
                        .command
                        .as_deref()
                        .ok_or_else(|| anyhow!("MCP stdio server missing command"))?,
                )
                .configure(|cmd| {
                    cmd.args(json_string_vec(&server.args));
                    for (key, value) in server.env_json.as_object().cloned().unwrap_or_default() {
                        if let Some(value) = value.as_str() {
                            cmd.env(key, value);
                        }
                    }
                }),
            )?;
            Ok(client_info().serve(transport).await?)
        }
        other => Err(anyhow!("unsupported MCP transport {other}")),
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
