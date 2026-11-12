use anyhow::anyhow;

use crate::{config::WorkerConfig, db};

use super::super::{
    RequestExecutionContext, context::RuntimeServices, discover_dynamic_model_route,
    materialize_route_api_key_selection_with_quota, request_assembly::BufferedBridgeRequest,
    routing::clear_invalid_conversation_endpoint_key_override, select_route_for_candidate,
};

pub(super) enum RouteResolution {
    Ready {
        route: Box<db::RouteConfig>,
        quota_probe: Option<crate::worker::runtime::routing::QuotaProbePermit>,
    },
}

pub(super) async fn resolve_route(
    request: &BufferedBridgeRequest,
    config: &WorkerConfig,
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
) -> anyhow::Result<RouteResolution> {
    if let Some(state) = services.standalone_state() {
        return Box::pin(resolve_standalone_route(
            request,
            config,
            services,
            request_ctx,
            state,
        ))
        .await;
    }
    let Some(state) = services.admin_state() else {
        return Box::pin(resolve_no_state_route(config, request)).await;
    };
    Box::pin(resolve_admin_route(
        request,
        config,
        services,
        request_ctx,
        state,
    ))
    .await
}

async fn resolve_standalone_route(
    request: &BufferedBridgeRequest,
    config: &WorkerConfig,
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    state: &crate::worker::runtime::standalone::StandaloneRuntimeState,
) -> anyhow::Result<RouteResolution> {
    let user_id = request_ctx.user_id.or(request.user_id).unwrap_or_default();
    let snapshot = state.snapshot().await;
    // Issue #1004 P1: the standalone deployment keeps its OAuth tokens in the
    // same SQLite store, so the token-id existence query hydrates the
    // non-secret subscription eligibility flag for the candidate targets.
    let subscription_endpoints = state
        .config_repository()
        .list_endpoint_oauth_token_ids()
        .await
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if let Some(candidate) = crate::worker::runtime::standalone::standalone_model_route_candidate(
        &snapshot,
        user_id,
        request_ctx.request_model.as_deref(),
        &subscription_endpoints,
    ) {
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
            quota_probe: selected.quota_probe,
        });
    }
    Ok(RouteResolution::Ready {
        route: Box::new(default_route_for_user(config, user_id)),
        quota_probe: None,
    })
}

async fn resolve_no_state_route(
    config: &WorkerConfig,
    request: &BufferedBridgeRequest,
) -> anyhow::Result<RouteResolution> {
    Ok(RouteResolution::Ready {
        route: Box::new(default_route(config, request)),
        quota_probe: None,
    })
}

async fn resolve_admin_route(
    request: &BufferedBridgeRequest,
    _config: &WorkerConfig,
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    state: &crate::worker_admin::AdminState,
) -> anyhow::Result<RouteResolution> {
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
            quota_probe: selected.quota_probe,
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
    state
        .token_plan_quota
        .hydrate_chatgpt_subscription(
            state.chatgpt_quota_service.as_deref(),
            route.route_id,
            route.provider,
        )
        .await;
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
    )?;
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
        quota_probe: key_selection.quota_probe,
    })
}

fn default_route(config: &WorkerConfig, request: &BufferedBridgeRequest) -> db::RouteConfig {
    default_route_for_user(config, request.user_id.unwrap_or_default())
}

fn default_route_for_user(config: &WorkerConfig, user_id: i64) -> db::RouteConfig {
    db::RouteConfig {
        embedding_dimensions_override: None,
        route_id: uuid::Uuid::nil(),
        user_id,
        model_route_rule_id: None,
        base_url: config.upstream_base_url.clone(),
        api_key: config.upstream_api_key.clone(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: config.upstream_native_api,
        upstream_model: None,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::Generic,
        service_tier: None,
        proxy_url: None,
        // Issue #392 Phase K: legacy direct routes never normalize.
        dev_system_normalize: false,
        // Issue #464: direct routes follow the caller.
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
        // Issue #1004 P1: a static upstream route never carries a ChatGPT
        // subscription credential.
        has_oauth_token: false,
    }
}
