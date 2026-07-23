use super::super::{
    RequestExecutionContext,
    context::{FailurePayload, RouteExecutionContext, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
};
use super::request_support::ai_route_usage_log;
use crate::{
    db,
    openai_compat::CompatError,
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseError, ResponseStart},
    worker_usage::record_usage_event,
};
use anyhow::Context;
use reqwest::StatusCode;

pub(super) async fn respond_with_local_error(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    failure: FailurePayload,
) -> anyhow::Result<()> {
    services
        .out_tx
        .send(BridgeMessage::ResponseError(ResponseError {
            request_id: request.request_id.clone(),
            status: failure.status.as_u16(),
            code: failure.error_code.clone(),
            message: failure.error_message.clone(),
        }))
        .context("relay response channel closed")?;
    record_usage_event(
        services.admin_state(),
        request_ctx
            .ai_usage_log(request, None)
            .with_upstream_redaction(
                request_ctx.request_prompt_log.upstream_redaction_enabled,
                request_ctx
                    .request_prompt_log
                    .upstream_redacted_request_json
                    .clone(),
                request_ctx
                    .request_prompt_log
                    .upstream_restore_session
                    .clone(),
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_status(
                Some(failure.status.as_u16() as i32),
                Some(false),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_error(
                Some(failure.error_code),
                Some(failure.error_message),
                failure.upstream_error_body,
            ),
    )
    .await;
    Ok(())
}

pub(super) async fn respond_with_client_error(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route_ctx: &RouteExecutionContext,
    err: CompatError,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "error": {
            "code": err.code,
            "message": err.message,
        }
    })
    .to_string();
    services
        .out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: err.status.as_u16(),
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: body.clone().into_bytes(),
        }))
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }))
        .context("relay response channel closed")?;
    record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, route_ctx)
            .with_upstream_redaction(
                request_ctx.request_prompt_log.upstream_redaction_enabled,
                request_ctx
                    .request_prompt_log
                    .upstream_redacted_request_json
                    .clone(),
                request_ctx
                    .request_prompt_log
                    .upstream_restore_session
                    .clone(),
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_status(
                Some(err.status.as_u16() as i32),
                Some(false),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_error(Some(err.code.to_string()), Some(err.message), Some(body)),
    )
    .await;
    Ok(())
}

pub(super) async fn respond_with_budget_error(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route_ctx: RouteExecutionContext,
    message: String,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "error": {
            "code": "budget_exceeded",
            "message": message,
        }
    })
    .to_string();
    services
        .out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: body.clone().into_bytes(),
        }))
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }))
        .context("relay response channel closed")?;
    record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, &route_ctx)
            .with_upstream_redaction(
                request_ctx.request_prompt_log.upstream_redaction_enabled,
                request_ctx
                    .request_prompt_log
                    .upstream_redacted_request_json
                    .clone(),
                request_ctx
                    .request_prompt_log
                    .upstream_restore_session
                    .clone(),
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_status(
                Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i32),
                Some(false),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_error(
                Some("budget_exceeded".to_string()),
                Some(message),
                Some(body),
            ),
    )
    .await;
    Ok(())
}
