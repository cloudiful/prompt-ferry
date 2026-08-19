use anyhow::anyhow;

use crate::{config::WorkerConfig, db};

use super::super::{
    RequestExecutionContext, check_named_request_budget,
    context::{RouteExecutionContext, RuntimeServices},
    discover_dynamic_model_route, materialize_route_api_key_selection_with_quota,
    request_assembly::BufferedBridgeRequest,
    routing::clear_invalid_conversation_endpoint_key_override,
    select_route_for_candidate,
};
use super::errors::respond_with_budget_error;

pub(super) enum RouteResolution {
    Ready { route: Box<db::RouteConfig> },
    Responded,
}

pub(super) async fn resolve_route(
    request: &BufferedBridgeRequest,
    config: &WorkerConfig,
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
) -> anyhow::Result<RouteResolution> {
    let Some(state) = services.admin_state() else {
        return Ok(RouteResolution::Ready {
            route: Box::new(default_route(config, request)),
        });
    };

    let user_id = request.user_id.unwrap_or_default();
    let whitelist_enabled = state
        .model_route_whitelist_enabled
        .load(std::sync::atomic::Ordering::SeqCst);
    let use_fallback = !whitelist_enabled;
    let (fallback_route, candidate) = db::resolve_model_route_with_fallback(
        &state.pool,
        user_id,
        request_ctx.request_model.as_deref(),
        use_fallback,
    )
    .await?;

    if let Some(candidate) = candidate {
        if let Some(message) = check_named_request_budget(
            &state.pool,
            db::RequestRecordCategory::Ai,
            db::RequestBudgetScope::ModelRoute(candidate.rule_id),
            "model route",
            &candidate.model_pattern,
            candidate.daily_max_requests,
            candidate.monthly_max_requests,
        )
        .await?
        {
            respond_with_budget_error(
                services,
                request,
                request_ctx,
                RouteExecutionContext {
                    route: db::RouteConfig {
                        route_id: uuid::Uuid::nil(),
                        user_id,
                        model_route_rule_id: Some(candidate.rule_id),
                        base_url: String::new(),
                        api_key: String::new(),
                        endpoint_key_id: None,
                        endpoint_key_label: None,
                        api_keys: Vec::new(),
                        key_lb_enabled: false,
                        native_api: config.upstream_native_api,
                        upstream_model: None,
                        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                        route_selection_reason: db::RouteSelectionReason::Default,
                    },
                    endpoint_id: None,
                    model_route_rule_id: Some(candidate.rule_id),
                    route_selection_reason: db::RouteSelectionReason::Default,
                },
                message,
            )
            .await?;
            return Ok(RouteResolution::Responded);
        }
        let selected = select_route_for_candidate(
            services,
            request_ctx,
            &candidate,
            request,
            user_id,
            request.client_key_hash.as_deref(),
        )
        .await?
        .ok_or_else(|| anyhow!("route not found"))?;
        return Ok(RouteResolution::Ready {
            route: Box::new(selected.route),
        });
    }

    let dynamic_route = if !whitelist_enabled {
        discover_dynamic_model_route(
            state,
            &services.client,
            user_id,
            request_ctx.request_model.as_deref(),
            fallback_route.as_ref(),
        )
        .await
    } else {
        None
    };

    let mut route = dynamic_route
        .or(fallback_route)
        .ok_or_else(|| anyhow!("route not found"))?;
    if let Err(err) = state
        .token_plan_quota
        .refresh_if_due(&state.pool, route.route_id)
        .await
    {
        tracing::warn!(
            endpoint_id = %route.route_id,
            error = %err,
            "MiniMax quota refresh failed for fallback route; retaining the previous snapshot"
        );
    }
    let key_selection = materialize_route_api_key_selection_with_quota(
        &route,
        request,
        &request_ctx.request_prompt_log,
        Some(&state.token_plan_quota),
    );
    clear_invalid_conversation_endpoint_key_override(
        services,
        &request_ctx.request_prompt_log,
        key_selection.invalid_conversation_override,
    )
    .await;
    route.api_key = key_selection.selection.secret;
    route.endpoint_key_id = key_selection.selection.key_id;
    route.endpoint_key_label = key_selection.selection.key_label;
    Ok(RouteResolution::Ready {
        route: Box::new(route),
    })
}

fn default_route(config: &WorkerConfig, request: &BufferedBridgeRequest) -> db::RouteConfig {
    db::RouteConfig {
        route_id: uuid::Uuid::nil(),
        user_id: request.user_id.unwrap_or_default(),
        model_route_rule_id: None,
        base_url: config.upstream_base_url.clone(),
        api_key: config.upstream_api_key.clone(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: config.upstream_native_api,
        upstream_model: None,
        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
    }
}
