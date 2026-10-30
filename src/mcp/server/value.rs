use rmcp::{
    ErrorData,
    model::{
        CallToolResponse, CompleteResult, GetPromptResponse, Implementation, InitializeResult,
        ReadResourceResponse, RequestId, RequestMetaObject, ResultType, ServerCapabilities,
        ServerConfig, ServerResult,
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::naming::MCP_SERVER_NAME;

use super::super::protocol::{DEFAULT_PROTOCOL_VERSION, DEFAULT_PROTOCOL_VERSION_STR};

pub(super) fn server_info() -> ServerConfig {
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

/// Preserve an upstream JSON-RPC error exactly: its `code`, `message`, and
/// `data` are part of the protocol contract and must survive the gateway. Only
/// `ServiceError::McpError` is an upstream JSON-RPC error; transport, parse,
/// and internal failures keep the existing internal-error envelope so they are
/// never misreported as an upstream error.
pub(super) fn dispatch_error(err: anyhow::Error) -> ErrorData {
    if let Some(rmcp::service::ServiceError::McpError(error_data)) =
        err.downcast_ref::<rmcp::service::ServiceError>()
    {
        return error_data.clone();
    }
    internal_error(err)
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
        ServerResult::CallToolResult(mut result) => {
            ensure_complete_result_type(&mut result.result_type);
            Ok(result.into())
        }
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("tools/call")),
        _ => Err(unexpected_result_error("tools/call")),
    }
}

pub(super) fn parse_get_prompt_response(response: Value) -> Result<GetPromptResponse, ErrorData> {
    match parse_server_result(response)? {
        ServerResult::GetPromptResult(mut result) => {
            ensure_complete_result_type(&mut result.result_type);
            Ok(result.into())
        }
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("prompts/get")),
        _ => Err(unexpected_result_error("prompts/get")),
    }
}

pub(super) fn parse_read_resource_response(
    response: Value,
) -> Result<ReadResourceResponse, ErrorData> {
    match parse_server_result(response)? {
        ServerResult::ReadResourceResult(mut result) => {
            ensure_complete_result_type(&mut result.result_type);
            Ok(result.into())
        }
        ServerResult::InputRequiredResult(result) => Ok(result.into()),
        ServerResult::CreateTaskResult(_) => Err(unsupported_tasks_error("resources/read")),
        _ => Err(unexpected_result_error("resources/read")),
    }
}

pub(super) fn parse_complete_result(response: Value) -> Result<CompleteResult, ErrorData> {
    let mut result: CompleteResult = parse_result(response)?;
    ensure_complete_result_type(&mut result.result_type);
    Ok(result)
}

/// `2026-07-28` requires `resultType` on ordinary results, and upstreams that
/// predate it (including the built-in MiniMax bridge) omit the field. An absent
/// field means "complete" per the spec, so normalize it only for a result that
/// is already a plain completion; MRTR and task variants never reach here.
fn ensure_complete_result_type(result_type: &mut Option<ResultType>) {
    if result_type.is_none() {
        *result_type = Some(ResultType::COMPLETE);
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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;

    #[test]
    fn dispatch_error_preserves_upstream_jsonrpc_error_code_message_and_data() {
        let upstream = ErrorData::new(
            ErrorCode(-32001),
            "legacy tool failed",
            Some(json!({"detail": "boom"})),
        );
        let err = anyhow::Error::new(rmcp::service::ServiceError::McpError(upstream));
        let mapped = dispatch_error(err);
        assert_eq!(mapped.code, ErrorCode(-32001));
        assert_eq!(mapped.message, "legacy tool failed");
        assert_eq!(mapped.data, Some(json!({"detail": "boom"})));
    }

    #[test]
    fn dispatch_error_does_not_report_transport_failure_as_upstream_error() {
        let mapped = dispatch_error(anyhow::Error::new(
            rmcp::service::ServiceError::TransportClosed,
        ));
        assert_eq!(mapped.code, ErrorCode::INTERNAL_ERROR);
        assert!(
            mapped.message.contains("Transport closed"),
            "unexpected message: {}",
            mapped.message
        );
        assert!(mapped.data.is_none());
    }

    #[test]
    fn dispatch_error_keeps_internal_envelope_for_non_jsonrpc_failure() {
        let mapped = dispatch_error(anyhow::anyhow!("connection reset"));
        assert_eq!(mapped.code, ErrorCode::INTERNAL_ERROR);
        assert!(
            mapped
                .message
                .contains("mcp protocol 2025-11-25: connection reset"),
            "unexpected message: {}",
            mapped.message
        );
        assert!(mapped.data.is_none());
    }
}
