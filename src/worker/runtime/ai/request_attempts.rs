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
    forward::{ResponseForwardContext, ResponseLoggingContext},
    request_logging::log_prepared_upstream_summary,
    request_support::prepare_upstream_request_with_replay,
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

pub(super) struct RouteForwardRequest<'a> {
    pub(super) services: &'a RuntimeServices,
    pub(super) request: &'a BufferedBridgeRequest,
    pub(super) request_ctx: &'a RequestExecutionContext,
    pub(super) route: &'a db::RouteConfig,
    pub(super) method: &'a Method,
    pub(super) redact_content: bool,
    pub(super) content_logging_enabled: bool,
    pub(super) raw_content_logging_enabled: bool,
}

pub(super) async fn forward_route_request(
    input: RouteForwardRequest<'_>,
) -> anyhow::Result<ForwardOutcome> {
    let RouteForwardRequest {
        services,
        request,
        request_ctx,
        route,
        method,
        redact_content,
        content_logging_enabled,
        raw_content_logging_enabled,
    } = input;
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
        request_ctx.request_prompt_log.replay_unavailable,
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
            db::RequestRecordStateInput {
                request_id: request_ctx.request_id,
                request_state: db::RequestRecordState::UpstreamProcessing,
                endpoint_id: Some(route.route_id).filter(|id| !id.is_nil()),
                model_route_rule_id: route.model_route_rule_id,
                model: request_ctx.request_model.as_deref(),
                endpoint_key_id: route.endpoint_key_id,
                endpoint_key_label: route.endpoint_key_label.as_deref(),
            },
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
    log_prepared_upstream_summary(route, &prepared);
    match response {
        Ok(response) => {
            handle_attempt_response(
                response,
                ResponseForwardContext {
                    route_ctx: &route_ctx,
                    request,
                    request_ctx,
                    upstream_redacted_request_json: prepared.upstream_redacted_request_json.clone(),
                    upstream_restore_session: prepared.upstream_restore_session.clone(),
                    logging: ResponseLoggingContext {
                        redact_content,
                        content_logging_enabled,
                        raw_content_logging_enabled,
                    },
                    response_adapter: prepared.response_adapter,
                    services,
                },
            )
            .await
        }
        Err(err) => Ok(ForwardOutcome::TransportError(
            anyhow!(err).context("upstream request failed"),
        )),
    }
}

async fn handle_attempt_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
) -> anyhow::Result<ForwardOutcome> {
    match forward_upstream_response(response, context).await {
        Ok(()) => Ok(ForwardOutcome::Handled),
        Err(err) => Ok(ForwardOutcome::TransportError(err)),
    }
}
