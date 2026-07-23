use anyhow::anyhow;
use http::Method;

use crate::{db, openai_compat::CompatError};

use super::super::{
    RequestExecutionContext, check_named_request_budget,
    context::{RouteExecutionContext, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
    upstream_url,
};
use super::{
    forward::forward_upstream_response,
    request_support::{log_prepared_upstream_request, prepare_upstream_request_with_replay},
    upstream::send_upstream_request,
};

pub(super) enum ForwardOutcome {
    Handled,
    CompatError(CompatError),
    BudgetError {
        endpoint_id: Option<uuid::Uuid>,
        message: String,
        model_route_rule_id: Option<uuid::Uuid>,
    },
    TransportError(anyhow::Error),
}

pub(super) async fn forward_route_request(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    method: &Method,
    redact_content: bool,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
) -> anyhow::Result<ForwardOutcome> {
    let route_ctx = RouteExecutionContext::new(route);
    if let Some(state) = services.admin_state()
        && !route.route_id.is_nil()
        && let Some(endpoint) = db::get_endpoint(&state.pool, route.route_id).await?
        && let Some(message) = check_named_request_budget(
            &state.pool,
            db::RequestRecordCategory::Ai,
            db::RequestBudgetScope::Endpoint(endpoint.endpoint_id),
            "endpoint",
            &endpoint.name,
            endpoint.daily_max_requests,
            endpoint.monthly_max_requests,
        )
        .await?
    {
        return Ok(ForwardOutcome::BudgetError {
            endpoint_id: Some(endpoint.endpoint_id),
            message,
            model_route_rule_id: route.model_route_rule_id,
        });
    }

    let prepared = match prepare_upstream_request_with_replay(
        services.admin_state(),
        route,
        request,
        request_ctx.request_prompt_log.conversation_id,
        request_ctx.request_prompt_log.parent_event_id,
        request_ctx.request_model.as_deref(),
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(err) => return Ok(ForwardOutcome::CompatError(err)),
    };
    let upstream_url = upstream_url(&route.base_url, &prepared.path);
    if let Some(state) = services.admin_state() {
        let _ = db::record_request_state(
            &state.pool,
            request_ctx.request_id,
            db::RequestRecordState::UpstreamProcessing,
            Some(route.route_id).filter(|id| !id.is_nil()),
            route.model_route_rule_id,
            request_ctx.request_model.as_deref(),
            route.endpoint_key_id,
            route.endpoint_key_label.as_deref(),
        )
        .await;
    }
    let response = send_upstream_request(
        &services.client,
        method,
        &upstream_url,
        route,
        &prepared.body,
    )
    .await;
    log_prepared_upstream_request(route, &request.path, &prepared.body);
    match response {
        Ok(response) => {
            handle_attempt_response(
                services,
                request,
                request_ctx,
                route_ctx,
                response,
                prepared.upstream_redacted_request_json.clone(),
                prepared.upstream_restore_session.clone(),
                redact_content,
                content_logging_enabled,
                raw_content_logging_enabled,
                prepared.response_adapter,
            )
            .await
        }
        Err(err) => Ok(ForwardOutcome::TransportError(
            anyhow!(err).context("upstream request failed"),
        )),
    }
}

async fn handle_attempt_response(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route_ctx: RouteExecutionContext,
    response: reqwest::Response,
    upstream_redacted_request_json: Option<serde_json::Value>,
    upstream_restore_session: Option<crate::redact_upstream::UpstreamRedactionSession>,
    redact_content: bool,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    response_adapter: crate::upstream_adapter::ResponseAdapter,
) -> anyhow::Result<ForwardOutcome> {
    forward_upstream_response(
        response,
        &route_ctx,
        request,
        request_ctx,
        upstream_redacted_request_json,
        upstream_restore_session,
        redact_content,
        content_logging_enabled,
        raw_content_logging_enabled,
        response_adapter,
        services,
    )
    .await?;
    Ok(ForwardOutcome::Handled)
}
