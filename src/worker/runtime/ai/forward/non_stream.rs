use super::super::super::{context::RuntimeServices, error_handling::maybe_redact_text};
use super::super::{
    artifact::{persist_assistant_artifact, resolve_assistant_artifact},
    errors::respond_with_client_error,
    request_attempts::{UpstreamAttemptFailure, UpstreamFailurePhase},
    request_support::ai_route_usage_log,
};
use crate::{
    anthropic_compat::anthropic_response_to_responses,
    chat_replay::{AssistantArtifactCapture, ResponsesArtifactCapture},
    db,
    openai_compat::{CompatError, chat_response_to_responses},
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    usage::UsageCapture,
    worker_usage::record_usage_event,
};
use anyhow::{Context, anyhow};
use futures::StreamExt;

use super::super::upstream_restore::restore_ai_response_json_blocking;
use super::ResponseForwardContext;

pub(super) async fn send_json_response(
    services: &RuntimeServices,
    request_id: &str,
    status: u16,
    body: Vec<u8>,
) -> anyhow::Result<()> {
    services
        .out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request_id.to_string(),
            status,
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request_id.to_string(),
            data: body,
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request_id.to_string(),
        }))
        .await
        .context("relay response channel closed")?;
    Ok(())
}

pub(super) async fn forward_non_stream_chat_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
    mut assistant_capture: Option<&mut AssistantArtifactCapture>,
) -> anyhow::Result<()> {
    let route_ctx = context.route_ctx;
    let request = context.request;
    let request_ctx = context.request_ctx;
    let upstream_redacted_request_json = context.upstream_redacted_request_json.clone();
    let upstream_restore_session = context.upstream_restore_session.clone();
    let redact_content = context.logging.redact_content;
    let content_logging_enabled = context.logging.content_logging_enabled;
    let raw_content_logging_enabled = context.logging.raw_content_logging_enabled;
    let services = context.services;
    let status = response.status();
    let body = read_response_limited(
        response,
        services.response_limits.max_upstream_response_bytes,
    )
    .await
    .map_err(map_body_read_error)?;
    if let Some(capture) = assistant_capture.as_mut() {
        capture.observe_chunk(&body);
        capture.finish();
    }
    let transformed = match chat_response_to_responses(&body) {
        Ok(transformed) => transformed,
        Err(err) => {
            let provider_response_id = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
            tracing::error!(
                request_id = %request.request_id,
                model = request_ctx.request_model.as_deref().unwrap_or("unknown"),
                tool_error = %err.message,
                provider_response_id = provider_response_id.as_deref().unwrap_or(""),
                streaming = false,
                "failed adapting invalid upstream tool call arguments"
            );
            return respond_with_client_error(
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
        restore_ai_response_json_blocking("/v1/responses".to_string(), transformed, session).await?
    } else {
        transformed
    };
    let mut capture = UsageCapture::new(false, request_ctx.request_model.clone());
    capture
        .set_response_text_capture_limit(services.response_limits.max_response_text_capture_bytes);
    let _ = capture.observe_chunk(&restored_body);
    capture.finish();
    let captured_artifact = assistant_capture
        .as_ref()
        .and_then(|capture| capture.artifact());
    let raw_body_truncated = body.len() > services.response_limits.max_raw_response_capture_bytes;
    let raw_body = &body[..body
        .len()
        .min(services.response_limits.max_raw_response_capture_bytes)];
    let (response_prompt, response_raw_body) = super::response_logging_payload(
        &capture.response_text,
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
                    maybe_redact_text(&capture.response_text, redact_content, request_ctx.user_id)
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
            .with_model(capture.model.clone())
            .with_status(
                Some(status.as_u16() as i32),
                Some(true),
                Some(request_ctx.elapsed_ms()),
                None,
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
                response_prompt.clone(),
                response_raw_body,
            )
            .with_response_capture_truncated(capture.response_text_truncated || raw_body_truncated),
    )
    .await;
    persist_assistant_artifact(
        services.admin_state(),
        usage_event_id,
        resolve_assistant_artifact(captured_artifact, None, artifact_response_text.as_deref()),
        request_ctx.request_prompt_log.conversation_id,
        request,
        &route_ctx.route,
        capture.response_id.as_deref(),
    )
    .await;
    Ok(())
}

pub(super) async fn forward_non_stream_responses_response(
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
    let services = context.services;
    let status = response.status();
    let body = read_response_limited(
        response,
        services.response_limits.max_upstream_response_bytes,
    )
    .await
    .map_err(map_body_read_error)?;
    responses_capture.observe_chunk(&body);
    responses_capture.finish();
    let restored_body = if let Some(session) = upstream_restore_session.clone() {
        restore_ai_response_json_blocking(request.path.clone(), body.to_vec(), session).await?
    } else {
        body.to_vec()
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

pub(super) async fn forward_non_stream_anthropic_response(
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
    let services = context.services;
    let status = response.status();
    let body = read_response_limited(
        response,
        services.response_limits.max_upstream_response_bytes,
    )
    .await
    .map_err(map_body_read_error)?;
    let transformed = anthropic_response_to_responses(&body)
        .map_err(|err| anyhow::anyhow!("failed translating anthropic response: {}", err.message))?;
    let restored_body = if let Some(session) = upstream_restore_session.clone() {
        restore_ai_response_json_blocking("/v1/responses".to_string(), transformed, session).await?
    } else {
        transformed
    };
    responses_capture.observe_chunk(&restored_body);
    responses_capture.finish();
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
        restored_body.clone(),
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

pub(super) enum UpstreamBodyReadError {
    Transport(reqwest::Error),
    TooLarge,
}

pub(super) fn map_body_read_error(err: UpstreamBodyReadError) -> anyhow::Error {
    match err {
        UpstreamBodyReadError::Transport(err) => {
            let retryable = UpstreamFailurePhase::BufferedResponseBody.is_transient(&err);
            UpstreamAttemptFailure {
                phase: UpstreamFailurePhase::BufferedResponseBody,
                error: anyhow!(err).context("failed reading upstream response"),
                retryable,
            }
            .into()
        }
        UpstreamBodyReadError::TooLarge => anyhow!("upstream_response_too_large"),
    }
}

pub(super) async fn read_response_limited(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, UpstreamBodyReadError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(UpstreamBodyReadError::Transport)?;
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|bytes| bytes > max_bytes)
        {
            return Err(UpstreamBodyReadError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
