use super::super::{
    RequestExecutionContext,
    context::{RouteExecutionContext, RuntimeServices},
    error_handling::ResponsesSseTerminal,
    request_assembly::BufferedBridgeRequest,
};
use super::request_support::ai_route_usage_log;
use crate::{
    db,
    protocol::{BridgeMessage, ResponseEnd, ResponseError},
    redact_upstream::UpstreamRedactionSession,
    usage::UsageCapture,
    worker_usage::record_usage_event,
};
use anyhow::Context;
use serde_json::Value;

pub(super) fn failure_details(
    terminal: Option<ResponsesSseTerminal>,
) -> (&'static str, &'static str) {
    match terminal {
        Some(ResponsesSseTerminal::Failed) => (
            "responses_response_failed",
            "upstream Responses response failed",
        ),
        Some(ResponsesSseTerminal::Incomplete) => (
            "responses_response_incomplete",
            "upstream Responses response incomplete",
        ),
        Some(ResponsesSseTerminal::Error) => (
            "responses_upstream_error",
            "upstream Responses stream emitted an error event",
        ),
        Some(ResponsesSseTerminal::Completed) => {
            ("responses_stream_completed", "Responses stream completed")
        }
        None => (
            "responses_stream_incomplete",
            "upstream Responses SSE stream ended before a terminal response event; the provider or an intermediate relay/proxy closed the stream early",
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish_failure(
    terminal: Option<ResponsesSseTerminal>,
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route_ctx: &RouteExecutionContext,
    status: u16,
    capture: &mut UsageCapture,
    raw_response_body: &[u8],
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    redact_content: bool,
    upstream_redacted_request_json: Option<Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    first_chunk_ms: Option<i64>,
) -> anyhow::Result<()> {
    let (code, message) = failure_details(terminal);
    capture.finish();
    let (response_prompt, response_raw_body) = super::forward::response_logging_payload(
        &capture.response_text,
        raw_response_body,
        content_logging_enabled,
        raw_content_logging_enabled,
        redact_content,
        request_ctx.user_id,
    );
    record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, route_ctx)
            .with_upstream_redaction(
                upstream_restore_session.is_some(),
                upstream_redacted_request_json,
                upstream_restore_session,
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_model(capture.model.clone())
            .with_status(
                Some(status as i32),
                Some(false),
                Some(request_ctx.elapsed_ms()),
                first_chunk_ms,
            )
            .with_usage(capture.usage.clone())
            .with_response(
                capture.response_id.clone(),
                capture.provider_conversation_key.clone().or_else(|| {
                    request_ctx
                        .request_prompt_log
                        .request_conversation_key
                        .clone()
                }),
                response_prompt,
                response_raw_body,
            )
            .with_error(Some(code.to_string()), Some(message.to_string()), None),
    )
    .await;

    if terminal.is_some() {
        services
            .out_tx
            .send(BridgeMessage::ResponseEnd(ResponseEnd {
                request_id: request.request_id.clone(),
            }))
            .context("relay response channel closed")?;
    } else {
        services
            .out_tx
            .send(BridgeMessage::ResponseError(ResponseError {
                request_id: request.request_id.clone(),
                status: http::StatusCode::BAD_GATEWAY.as_u16(),
                code: code.to_string(),
                message: message.to_string(),
            }))
            .context("relay response channel closed")?;
    }
    Ok(())
}
