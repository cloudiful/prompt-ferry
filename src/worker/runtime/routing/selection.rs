use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use super::key_pool;
use super::quota_selection::{refresh_candidate_quota, request_model};
use crate::{
    db, endpoint_models,
    worker_admin::{
        AdminState,
        token_plan_cache::{TokenPlanQuotaCache, estimate_input_tokens},
    },
};
use reqwest::Client;
use tracing::{info, warn};

pub(in crate::worker::runtime) struct SelectedRoute {
    pub(in crate::worker::runtime) route: db::RouteConfig,
}

struct CandidateSelection<'a> {
    target: &'a db::ModelRouteCandidateTarget,
    key: db::EndpointApiKeySelection,
    reason: db::RouteSelectionReason,
    invalid_override: bool,
}

pub(in crate::worker::runtime) async fn discover_dynamic_model_route(
    state: &AdminState,
    client: &Client,
    user_id: i64,
    request_model: Option<&str>,
    fallback_route: Option<&db::RouteConfig>,
) -> Option<db::RouteConfig> {
    let model = request_model?;
    let visible_routes = match db::list_visible_endpoints(&state.pool, user_id).await {
        Ok(routes) => routes,
        Err(err) => {
            warn!(
                user_id,
                model,
                error = %err,
                "failed to list visible endpoints for model discovery"
            );
            return None;
        }
    };
    if visible_routes.is_empty() {
        return None;
    }

    let discovered = endpoint_models::discover_route_for_model(
        &state.endpoint_model_cache,
        &visible_routes,
        fallback_route,
        model,
        |route| {
            let client = client.clone();
            let route = route.clone();
            async move { endpoint_models::fetch_endpoint_model_ids(&client, &route).await }
        },
    )
    .await;

    if let Some(route) = &discovered
        && fallback_route.is_none_or(|fallback| fallback.route_id != route.route_id)
    {
        info!(
            user_id,
            model,
            endpoint_id = %route.route_id,
            "selected endpoint via dynamic model discovery"
        );
    }

    discovered
}

/// Unified routing entry: one eligibility filter and one deterministic
/// weighted draw over the whole candidate key pool. Session affinity keeps
/// its binding shape (`endpoint_id` + key) and pins the drawn unit.
pub(in crate::worker::runtime) async fn select_route_for_candidate(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    candidate: &db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    user_id: i64,
    routing_key: Option<&str>,
) -> anyhow::Result<Option<SelectedRoute>> {
    if services.standalone_state().is_none()
        && candidate.routing_strategy == db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
        && (request.path == "/v1/responses" || request.path == "/v1/chat/completions")
    {
        let selected =
            super::session_affinity::select(services, request_ctx, candidate, request, user_id)
                .await?;
        return Ok(Some(SelectedRoute {
            route: route_from_target(
                selected.target,
                user_id,
                candidate.rule_id,
                selected.key_selection,
                selected.route_selection_reason,
            ),
        }));
    }

    let request_model = request_model(request);
    let quota_cache = services.admin_state().map(|state| &state.token_plan_quota);
    refresh_candidate_quota(services, candidate).await;
    let Some(selected) = select_unified_candidate(
        candidate,
        request,
        &request_ctx.request_prompt_log,
        quota_cache,
        request_model.as_deref(),
        routing_key,
    ) else {
        return Ok(None);
    };
    clear_invalid_conversation_endpoint_key_override(
        services,
        &request_ctx.request_prompt_log,
        selected.invalid_override,
    )
    .await;
    Ok(Some(SelectedRoute {
        route: route_from_target(
            selected.target,
            user_id,
            candidate.rule_id,
            selected.key,
            selected.reason,
        ),
    }))
}

fn select_unified_candidate<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    routing_key: Option<&str>,
) -> Option<CandidateSelection<'a>> {
    let stable_key = routing_stable_key(request, request_prompt_log, routing_key);
    let estimated = estimate_input_tokens(&request.body);

    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id
        && let Some(target) = candidate
            .targets
            .iter()
            .find(|target| target.enabled && target.endpoint_id == endpoint_id)
    {
        let units = key_pool::target_units(target, quota_cache, model);
        let drawn = key_pool::draw(&units, &stable_key, quota_cache, estimated);
        let key = drawn
            .map(|unit| unit.selection())
            .unwrap_or_else(|| target_secret(target));
        let (key, invalid_override) = key_pool::apply_override(
            target,
            key,
            request_prompt_log.conversation_override_endpoint_key_id,
        );
        return Some(CandidateSelection {
            target,
            key,
            reason: db::RouteSelectionReason::ConversationOverride,
            invalid_override,
        });
    }

    let mut units = key_pool::candidate_units(candidate, quota_cache, model, None);
    if units.is_empty() {
        units = key_pool::candidate_units_without_quota(candidate, None);
    }
    let unit = key_pool::draw(&units, &stable_key, quota_cache, estimated)?;
    let (key, invalid_override) = key_pool::apply_override(
        unit.target,
        unit.selection(),
        request_prompt_log.conversation_override_endpoint_key_id,
    );
    Some(CandidateSelection {
        target: unit.target,
        key,
        reason: db::RouteSelectionReason::Default,
        invalid_override,
    })
}

fn target_secret(target: &db::ModelRouteCandidateTarget) -> db::EndpointApiKeySelection {
    db::EndpointApiKeySelection {
        key_id: None,
        key_label: None,
        secret: target.api_key.clone(),
    }
}

/// Session identity first (conversation / previous response / provider
/// conversation / session header), then the caller-supplied routing key
/// (the client key for rendezvous routing), then a stable constant.
fn routing_stable_key(
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    routing_key: Option<&str>,
) -> String {
    endpoint_key_stickiness_value(request, request_prompt_log)
        .or_else(|| {
            routing_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string())
}

fn route_from_target(
    target: &db::ModelRouteCandidateTarget,
    user_id: i64,
    rule_id: uuid::Uuid,
    key_selection: db::EndpointApiKeySelection,
    route_selection_reason: db::RouteSelectionReason,
) -> db::RouteConfig {
    db::RouteConfig {
        route_id: target.endpoint_id,
        user_id,
        model_route_rule_id: Some(rule_id),
        base_url: target.base_url.clone(),
        api_key: key_selection.secret,
        endpoint_key_id: key_selection.key_id,
        endpoint_key_label: key_selection.key_label,
        api_keys: target.api_keys.clone(),
        key_lb_enabled: target.key_lb_enabled,
        native_api: target.native_api,
        upstream_model: target.upstream_model.clone(),
        route_selection_reason,
        provider: target.provider,
        service_tier: target.service_tier,
    }
}

pub(super) fn endpoint_key_stickiness_value(
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> Option<String> {
    if let Some(conversation_id) = request_prompt_log.conversation_id {
        return Some(format!("conversation:{conversation_id}"));
    }
    if let Some(previous_response_id) = request_prompt_log
        .request_previous_response_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("previous_response_id:{previous_response_id}"));
    }
    if let Some(conversation_key) = request_prompt_log
        .request_conversation_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("provider_conversation:{conversation_key}"));
    }
    if let Some(session_header_id) = request_prompt_log
        .session_header_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("session_header:{session_header_id}"));
    }
    request
        .client_key_hash
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("client_key:{value}"))
}

#[cfg(test)]
pub(in crate::worker::runtime) fn materialize_route_api_key_selection(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> EndpointApiKeySelectionResult {
    materialize_route_api_key_selection_with_quota(route, request, request_prompt_log, None)
}

pub(in crate::worker::runtime) fn materialize_route_api_key_selection_with_quota(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
) -> EndpointApiKeySelectionResult {
    let model = request_model(request);
    let model = model.as_deref();
    if let Some(override_key_id) = request_prompt_log.conversation_override_endpoint_key_id {
        if let Some(selection) = key_pool::override_route_key(route, override_key_id) {
            return EndpointApiKeySelectionResult {
                selection,
                invalid_conversation_override: false,
            };
        }
        let (selection, _) = draw_route_key(route, request, request_prompt_log, quota_cache, model);
        return EndpointApiKeySelectionResult {
            selection,
            invalid_conversation_override: true,
        };
    }
    let (selection, _) = draw_route_key(route, request, request_prompt_log, quota_cache, model);
    EndpointApiKeySelectionResult {
        selection,
        invalid_conversation_override: false,
    }
}

fn draw_route_key(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
) -> (db::EndpointApiKeySelection, bool) {
    let units = key_pool::route_units(route, quota_cache, model);
    let stable_key = endpoint_key_stickiness_value(request, request_prompt_log);
    let estimated = estimate_input_tokens(&request.body);
    if let Some(stable_key) = stable_key
        && let Some(unit) =
            key_pool::draw_route(&units, &stable_key, route.route_id, quota_cache, estimated)
    {
        return (unit.key.clone(), false);
    }
    units
        .first()
        .map(|unit| (unit.key.clone(), false))
        .unwrap_or_else(|| {
            (
                db::EndpointApiKeySelection {
                    key_id: None,
                    key_label: None,
                    secret: route.api_key.clone(),
                },
                false,
            )
        })
}

pub(in crate::worker::runtime) async fn clear_invalid_conversation_endpoint_key_override(
    services: &RuntimeServices,
    request_prompt_log: &RequestPromptLog,
    invalid: bool,
) {
    if !invalid {
        return;
    }
    let (Some(admin_state), Some(conversation_id)) =
        (services.admin_state(), request_prompt_log.conversation_id)
    else {
        return;
    };
    if let Err(err) =
        db::clear_conversation_endpoint_key_override(&admin_state.pool, conversation_id).await
    {
        warn!(
            error = %err,
            conversation_id = %conversation_id,
            "failed to clear invalid conversation endpoint key override"
        );
    }
}

pub(in crate::worker::runtime) struct EndpointApiKeySelectionResult {
    pub(in crate::worker::runtime) selection: db::EndpointApiKeySelection,
    pub(in crate::worker::runtime) invalid_conversation_override: bool,
}
