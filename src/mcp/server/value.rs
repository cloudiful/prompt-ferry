use rmcp::{
    ErrorData,
    model::{Implementation, InitializeResult, RequestId, ServerCapabilities, ServerInfo},
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

fn request_id_to_value(request_id: &RequestId) -> Value {
    serde_json::to_value(request_id).unwrap_or_else(|_| json!("proxy"))
}
