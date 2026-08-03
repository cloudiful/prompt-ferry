use super::super::super::{context::FailurePayload, record_mcp_request_event};
use reqwest::StatusCode;

use crate::worker::runtime::mcp_support::McpResponseContext;

use super::send_mcp_response;

pub(super) async fn handle_restore_failure(
    context: &McpResponseContext<'_>,
    err: anyhow::Error,
    upstream_body: Vec<u8>,
) {
    let client_body = serde_json::json!({
        "error": {
            "code": "upstream_restore_failed",
            "message": format!("failed to restore upstream MCP response: {err}"),
        }
    })
    .to_string()
    .into_bytes();
    send_mcp_response(
        context.services,
        &context.request.request_id,
        StatusCode::BAD_GATEWAY.as_u16(),
        Some("application/json".to_string()),
        Vec::new(),
        client_body,
    )
    .await;
    record_mcp_request_event(
        context,
        FailurePayload {
            status: StatusCode::BAD_GATEWAY,
            error_code: "upstream_restore_failed".to_string(),
            error_message: err.to_string(),
            upstream_error_body: Some(String::from_utf8_lossy(&upstream_body).to_string()),
            response_body: None,
        },
    )
    .await;
}
