use super::super::super::{context::RuntimeServices, error_handling::maybe_redact_text};
use super::super::{
    artifact::{persist_assistant_artifact, resolve_assistant_artifact},
    request_support::ai_route_usage_log,
};
use super::ResponseForwardContext;
use super::non_stream::{read_response_limited, send_json_response};
use crate::{
    chat_replay::ResponsesArtifactCapture,
    db,
    openai_compat::{CompatError, responses_response_to_chat},
    usage::UsageCapture,
    worker_usage::record_usage_event,
};

pub(super) async fn forward_non_stream_responses_to_chat_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
    responses_capture: &mut ResponsesArtifactCapture,
) -> anyhow::Result<()> {
    let route_ctx = context.route_ctx;
    let request = context.request;
    let request_ctx = context.request_ctx;
    let upstream_redacted_request_json = context.upstream_redacted_request_json.clone();
    let upstream_restore_session = context.upstream_restore_session.clone();
    let redact_content = context.logging.redact_content;
    let content_logging_enabled = context.logging.content_logging_enabled;
    let raw_content_logging_enabled = context.logging.raw_content_logging_enabled;
    let services: &RuntimeServices = context.services;
    let status = response.status();
    let body = read_response_limited(
        response,
        services.response_limits.max_upstream_response_bytes,
    )
    .await?;
    responses_capture.observe_chunk(&body);
    responses_capture.finish();

    let translated = match responses_response_to_chat(&body) {
        Ok(translated) => translated,
        Err(err) => {
            return super::super::errors::respond_with_client_error(
                services,
                request,
                request_ctx,
                route_ctx,
                CompatError::new(reqwest::StatusCode::BAD_GATEWAY, err.code, err.message),
            )
            .await;
        }
    };
    let restored_body = if let Some(session) = upstream_restore_session.clone() {
        super::super::upstream_restore::restore_ai_response_json_blocking(
            request.path.clone(),
            translated,
            session,
        )
        .await?
    } else {
        translated
    };

    let mut usage_capture = UsageCapture::new(false, request_ctx.request_model.clone());
    usage_capture
        .set_response_text_capture_limit(services.response_limits.max_response_text_capture_bytes);
    let _ = usage_capture.observe_chunk(&restored_body);
    usage_capture.finish();
    let captured_artifact = responses_capture.artifact();
    let raw_body_truncated = body.len() > services.response_limits.max_raw_response_capture_bytes;
    let raw_body = &body[..body
        .len()
        .min(services.response_limits.max_raw_response_capture_bytes)];
    let (response_prompt, response_raw_body) = super::response_logging_payload(
        &usage_capture.response_text,
        raw_body,
        content_logging_enabled,
        raw_content_logging_enabled,
        redact_content,
        request_ctx.user_id,
    );
    let artifact_response_text = if captured_artifact.is_none() {
        response_prompt.clone().or_else(|| {
            content_logging_enabled
                .then(|| {
                    maybe_redact_text(
                        &usage_capture.response_text,
                        redact_content,
                        request_ctx.user_id,
                    )
                })
                .filter(|text| !text.is_empty())
        })
    } else {
        None
    };
    send_json_response(
        services,
        &request.request_id,
        status.as_u16(),
        restored_body,
    )
    .await?;

    let usage_event_id = record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, route_ctx)
            .with_upstream_redaction(
                upstream_restore_session.is_some(),
                upstream_redacted_request_json,
                upstream_restore_session,
            )
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_model(usage_capture.model.clone())
            .with_status(
                Some(status.as_u16() as i32),
                Some(true),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_usage(usage_capture.usage.clone())
            .with_response(
                usage_capture.response_id.clone(),
                usage_capture.provider_conversation_key.clone().or_else(|| {
                    request_ctx
                        .request_prompt_log
                        .request_conversation_key
                        .clone()
                }),
                response_prompt.clone(),
                response_raw_body,
            )
            .with_response_capture_truncated(
                usage_capture.response_text_truncated || raw_body_truncated,
            ),
    )
    .await;
    persist_assistant_artifact(
        services.admin_state(),
        usage_event_id,
        resolve_assistant_artifact(captured_artifact, None, artifact_response_text.as_deref()),
        request_ctx.request_prompt_log.conversation_id,
        request,
        &route_ctx.route,
        usage_capture.response_id.as_deref(),
    )
    .await;
    Ok(())
}
