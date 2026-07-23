use super::super::super::{
    context::{FailurePayload, RuntimeServices},
    record_mcp_request_event,
};
use crate::{
    db, mcp::targeting::McpRequestMetadata, redact_upstream::UpstreamRedactionSession,
    worker_admin_types::RequestContentLoggingResponse,
};
use reqwest::StatusCode;

use super::{RequestExecutionContext, send_mcp_response};

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_restore_failure(
    request: &super::BufferedMcpRequest,
    request_ctx: &RequestExecutionContext,
    metadata: &McpRequestMetadata,
    request_content_logging: &RequestContentLoggingResponse,
    redact_content: bool,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    selected_token_slot: Option<i16>,
    server: Option<&db::McpServer>,
    err: anyhow::Error,
    upstream_body: Vec<u8>,
    services: &RuntimeServices,
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
        services,
        &request.request_id,
        StatusCode::BAD_GATEWAY.as_u16(),
        Some("application/json".to_string()),
        Vec::new(),
        client_body,
    );
    record_mcp_request_event(
        request_ctx,
        request,
        metadata,
        request_content_logging,
        redact_content,
        upstream_redacted_request_json,
        upstream_restore_session,
        selected_token_slot,
        server,
        FailurePayload {
            status: StatusCode::BAD_GATEWAY,
            error_code: "upstream_restore_failed".to_string(),
            error_message: err.to_string(),
            upstream_error_body: Some(String::from_utf8_lossy(&upstream_body).to_string()),
            response_body: None,
        },
        services,
    )
    .await;
}
