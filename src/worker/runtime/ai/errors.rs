use super::super::{
    RequestExecutionContext,
    context::{FailurePayload, RouteExecutionContext, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
    routing::RouteAffinityError,
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
        .await
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

pub(super) async fn respond_with_affinity_error(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    affinity_error: RouteAffinityError,
) -> anyhow::Result<()> {
    services
        .out_tx
        .send(BridgeMessage::ResponseError(ResponseError {
            request_id: request.request_id.clone(),
            status: affinity_error.status.as_u16(),
            code: affinity_error.code.to_string(),
            message: affinity_error.message.to_string(),
        }))
        .await
        .context("relay response channel closed")?;
    let audit = &affinity_error.audit;
    let (recorded_endpoint_id, recorded_key_id) =
        if affinity_error.code == "responses_session_affinity_conflict" {
            (audit.requested_endpoint_id, audit.requested_key_id)
        } else {
            (audit.endpoint_id, audit.endpoint_key_id)
        };
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
                Some(affinity_error.status.as_u16() as i32),
                Some(false),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_route(recorded_endpoint_id, audit.model_route_rule_id)
            .with_endpoint_key(recorded_key_id, None)
            .with_route_selection(db::RouteSelectionReason::SessionAffinity)
            .with_error(
                Some(affinity_error.code.to_string()),
                Some(affinity_error.message.to_string()),
                None,
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
    let body = if request.path == "/v1/messages" {
        serde_json::json!({
            "type": "error",
            "error": {
                "type": anthropic_error_type(err.status, &err.code),
                "message": err.message,
            },
        })
    } else {
        serde_json::json!({
            "error": {
                "code": err.code,
                "message": err.message,
            }
        })
    }
    .to_string();
    services
        .out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: err.status.as_u16(),
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: body.clone().into_bytes(),
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }))
        .await
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

fn anthropic_error_type(status: StatusCode, code: &str) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "authentication_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        status if status.is_server_error() => "api_error",
        _ if code == "permission_error" => "permission_error",
        _ if code == "not_found_error" => "not_found_error",
        _ => "invalid_request_error",
    }
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
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: body.clone().into_bytes(),
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }))
        .await
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
