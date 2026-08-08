use rmcp::{
    ErrorData,
    model::{
        CallToolResponse, GetPromptResponse, Implementation, InitializeResult,
        ReadResourceResponse, RequestId, RequestMetaObject, ServerCapabilities, ServerInfo,
        ServerResult,
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::naming::MCP_SERVER_NAME;

use super::super::protocol::{DEFAULT_PROTOCOL_VERSION, DEFAULT_PROTOCOL_VERSION_STR};

pub(super) fn server_info() -> ServerInfo {
    let capabilities = ServerCapabilities::builder()
        .enable_tools()
        .enable_resources()
        .enable_prompts()
        .enable_completions()
        .build();
    InitializeResult::new(capabilities)
        .with_protocol_version(DEFAULT_PROTOCOL_VERSION)
        .with_server_info(Implementation::new(
            MCP_SERVER_NAME,
            env!("CARGO_PKG_VERSION"),
        ))
}

pub(super) fn json_request(request_id: &RequestId, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id_to_value(request_id),
        "method": method,
        "params": params,
    })
}

pub(super) fn optional_params<T: serde::Serialize>(params: Option<T>) -> Result<Value, ErrorData> {
    params
        .map(serde_json::to_value)
        .transpose()
        .map(|value| value.unwrap_or_else(|| json!({})))
        .map_err(internal_error)
}

pub(super) fn required_params<T: serde::Serialize>(params: T) -> Result<Value, ErrorData> {
    serde_json::to_value(params).map_err(internal_error)
}

pub(super) fn parse_result<T: DeserializeOwned>(response: Value) -> Result<T, ErrorData> {
    let value = response
        .get("result")
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("missing MCP result payload", None))?;
    serde_json::from_value(value).map_err(internal_error)
}
pub(super) fn parse_result_field<T: DeserializeOwned>(
    response: &Value,
    field: &str,
) -> Result<T, ErrorData> {
    let value = response
        .pointer(&format!("/result/{field}"))
        .cloned()
        .ok_or_else(|| ErrorData::internal_error(format!("missing MCP result.{field}"), None))?;
    serde_json::from_value(value).map_err(internal_error)
}

pub(super) fn internal_error(err: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(
        format!("mcp protocol {DEFAULT_PROTOCOL_VERSION_STR}: {err}"),
        None,
    )
}

/// Attach the downstream request's `_meta` to the outgoing typed params.
/// Transport-level keys (`protocolVersion`, `clientInfo`, ...) are stripped by
/// the outbound connection, which regenerates them from its own identity.
pub(super) fn with_meta<P: rmcp::model::RequestParamsMeta>(
    params: P,
    meta: RequestMetaObject,
) -> P {
    let mut params = params;
    if !meta.is_empty() {
        params.set_meta(meta);
    }
    params
}

fn parse_server_result(response: Value) -> Result<ServerResult, ErrorData> {
    let value = response
        .get("result")
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("missing MCP result payload", None))?;
    serde_json::from_value(value).map_err(internal_error)
}

pub(super) fn parse_call_tool_response(response: Value) -> Result<CallToolResponse, ErrorData> {
    match parse_server_result(response)? {
        ServerResult::CallToolResult(result) => Ok(result.into()),
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("tools/call")),
        _ => Err(unexpected_result_error("tools/call")),
    }
}

pub(super) fn parse_get_prompt_response(response: Value) -> Result<GetPromptResponse, ErrorData> {
    match parse_server_result(response)? {
        ServerResult::GetPromptResult(result) => Ok(result.into()),
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("prompts/get")),
        _ => Err(unexpected_result_error("prompts/get")),
    }
}

pub(super) fn parse_read_resource_response(
    response: Value,
) -> Result<ReadResourceResponse, ErrorData> {
    match parse_server_result(response)? {
        ServerResult::ReadResourceResult(result) => Ok(result.into()),
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("resources/read")),
        _ => Err(unexpected_result_error("resources/read")),
    }
}

/// The proxy does not implement the SEP-2663 tasks extension; a task result
/// must never degrade into an empty result or a generic deserialize error.
fn unsupported_tasks_error(method: &str) -> ErrorData {
    ErrorData::internal_error(
        format!(
            "upstream returned a task for {method}, but the prompt-ferry MCP proxy does not support the tasks extension"
        ),
        None,
    )
}

fn unexpected_result_error(method: &str) -> ErrorData {
    ErrorData::internal_error(
        format!("upstream returned an unexpected result type for {method}"),
        None,
    )
}

fn request_id_to_value(request_id: &RequestId) -> Value {
    serde_json::to_value(request_id).unwrap_or_else(|_| json!("proxy"))
}
