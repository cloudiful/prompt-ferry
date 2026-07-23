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

pub(super) struct McpResponseContext<'a> {
    pub(super) request: &'a BufferedMcpRequest,
    pub(super) request_ctx: &'a RequestExecutionContext,
    pub(super) metadata: &'a McpRequestMetadata,
    pub(super) request_content_logging: &'a RequestContentLoggingResponse,
    pub(super) redact_content: bool,
    pub(super) upstream_redacted_request_json: Option<serde_json::Value>,
    pub(super) upstream_restore_session: Option<UpstreamRedactionSession>,
    pub(super) selected_token_slot: Option<i16>,
    pub(super) server: Option<&'a db::McpServer>,
    pub(super) services: &'a RuntimeServices,
}

pub(super) async fn record_mcp_request_event(
    context: &McpResponseContext<'_>,
    failure: FailurePayload,
) {
    let FailurePayload {
        status,
        error_code,
        error_message,
        upstream_error_body,
        response_body,
    } = failure;
    let ok = status.is_success();
    let response_body = context
        .request_content_logging
        .mode
        .captures_normalized()
        .then(|| {
            response_body
                .as_deref()
                .map(|text| {
                    maybe_redact_text(text, context.redact_content, context.request_ctx.user_id)
                })
                .filter(|text| !text.trim().is_empty())
        })
        .flatten();
    record_usage_event(
        context.services.admin_state(),
        context
            .request_ctx
            .mcp_usage_log(
                context.request,
                context.metadata,
                context.request_content_logging,
                context.redact_content,
            )
            .with_upstream_redaction(
                context.upstream_restore_session.is_some(),
                context.upstream_redacted_request_json.clone(),
                context.upstream_restore_session.clone(),
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
                context.server.map(|value| value.server_id),
                context
                    .server
                    .map(|value| value.name.clone())
                    .or_else(|| context.metadata.server_name.clone()),
                context.metadata.protocol_method.clone(),
                context.metadata.operation_name.clone(),
            )
            .with_mcp_token_slot(
                context
                    .selected_token_slot
                    .or(context.metadata.selected_token_slot),
            )
            .with_status(
                Some(status.as_u16() as i32),
                Some(ok),
                Some(context.request_ctx.elapsed_ms()),
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
