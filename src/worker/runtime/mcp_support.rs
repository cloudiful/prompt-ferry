use crate::{
    db, mcp::targeting::McpRequestMetadata, redact_upstream::UpstreamRedactionSession,
    worker_admin_types::RequestContentLoggingResponse, worker_usage::record_usage_event,
};

use super::{
    RequestExecutionContext,
    context::{FailurePayload, RuntimeServices},
    error_handling::maybe_redact_text,
    request_assembly::BufferedMcpRequest,
};

pub(super) async fn record_mcp_request_event(
    request_ctx: &RequestExecutionContext,
    request: &BufferedMcpRequest,
    metadata: &McpRequestMetadata,
    request_content_logging: &RequestContentLoggingResponse,
    redact_content: bool,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    selected_token_slot: Option<i16>,
    server: Option<&db::McpServer>,
    failure: FailurePayload,
    services: &RuntimeServices,
) {
    let FailurePayload {
        status,
        error_code,
        error_message,
        upstream_error_body,
        response_body,
    } = failure;
    let ok = status.is_success();
    let response_body = request_content_logging
        .mode
        .captures_normalized()
        .then(|| {
            response_body
                .as_deref()
                .map(|text| maybe_redact_text(text, redact_content, request_ctx.user_id))
                .filter(|text| !text.trim().is_empty())
        })
        .flatten();
    record_usage_event(
        services.admin_state(),
        request_ctx
            .mcp_usage_log(request, metadata, request_content_logging, redact_content)
            .with_upstream_redaction(
                upstream_restore_session.is_some(),
                upstream_redacted_request_json,
                upstream_restore_session,
            )
            .with_state(
                db::UsageEventKind::Request,
                if ok {
                    db::RequestRecordState::Completed
                } else {
                    db::RequestRecordState::Failed
                },
            )
            .with_mcp_context(
                server.map(|value| value.server_id),
                server
                    .map(|value| value.name.clone())
                    .or_else(|| metadata.server_name.clone()),
                metadata.protocol_method.clone(),
                metadata.operation_name.clone(),
            )
            .with_mcp_token_slot(selected_token_slot.or(metadata.selected_token_slot))
            .with_status(
                Some(status.as_u16() as i32),
                Some(ok),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_response(None, None, response_body, None)
            .with_error(
                (!error_code.is_empty()).then_some(error_code),
                (!error_message.is_empty()).then_some(error_message),
                upstream_error_body,
            ),
    )
    .await;
}
