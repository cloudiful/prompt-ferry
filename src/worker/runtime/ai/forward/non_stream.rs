use super::super::super::{
    RequestExecutionContext,
    context::{RouteExecutionContext, RuntimeServices},
    error_handling::maybe_redact_text,
    request_assembly::BufferedBridgeRequest,
};
use super::super::{
    artifact::{persist_assistant_artifact, resolve_assistant_artifact},
    errors::respond_with_client_error,
    request_support::ai_route_usage_log,
};
use crate::{
    anthropic_compat::anthropic_response_to_responses,
    chat_replay::{AssistantArtifactCapture, ResponsesArtifactCapture},
    db,
    openai_compat::{CompatError, chat_response_to_responses},
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    redact_upstream::UpstreamRedactionSession,
    usage::UsageCapture,
    worker_usage::record_usage_event,
};
use anyhow::Context;

use super::super::upstream_restore::restore_ai_response_json_blocking;

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
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request_id.to_string(),
            data: body,
        }))
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request_id.to_string(),
        }))
        .context("relay response channel closed")?;
    Ok(())
}

pub(super) async fn forward_non_stream_chat_response(
    response: reqwest::Response,
    route_ctx: &RouteExecutionContext,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    redact_content: bool,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    services: &RuntimeServices,
    mut assistant_capture: Option<&mut AssistantArtifactCapture>,
) -> anyhow::Result<()> {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
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
    let _ = capture.observe_chunk(&restored_body);
    capture.finish();
    let captured_artifact = assistant_capture
        .as_ref()
        .and_then(|capture| capture.artifact());
    let (response_prompt, response_raw_body) = super::response_logging_payload(
        &capture.response_text,
        &body,
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
        capture.response_id.as_deref(),
    )
    .await;
    Ok(())
}

pub(super) async fn forward_non_stream_responses_response(
    response: reqwest::Response,
    route_ctx: &RouteExecutionContext,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    redact_content: bool,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    services: &RuntimeServices,
    responses_capture: &mut ResponsesArtifactCapture,
) -> anyhow::Result<()> {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    responses_capture.observe_chunk(&body);
    responses_capture.finish();
    let restored_body = if let Some(session) = upstream_restore_session.clone() {
        restore_ai_response_json_blocking(request.path.clone(), body.to_vec(), session).await?
    } else {
        body.to_vec()
    };
    let mut usage_capture = UsageCapture::new(false, request_ctx.request_model.clone());
    let _ = usage_capture.observe_chunk(&restored_body);
    usage_capture.finish();
    let captured_artifact = responses_capture.artifact();
    let (response_prompt, response_raw_body) = super::response_logging_payload(
        &usage_capture.response_text,
        &body,
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
    route_ctx: &RouteExecutionContext,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<UpstreamRedactionSession>,
    redact_content: bool,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    services: &RuntimeServices,
    responses_capture: &mut ResponsesArtifactCapture,
) -> anyhow::Result<()> {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
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
    let _ = usage_capture.observe_chunk(&restored_body);
    usage_capture.finish();
    let captured_artifact = responses_capture.artifact();
    let (response_prompt, response_raw_body) = super::response_logging_payload(
        &usage_capture.response_text,
        &body,
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
