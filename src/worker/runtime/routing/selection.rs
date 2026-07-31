use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use crate::{
    db, endpoint_models,
    response_affinity::{ResponseAffinityBinding, api_key_fingerprint},
    routing::stable_candidate_order,
    worker_admin::AdminState,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreferredRouteReason {
    ConversationOverride,
    Rendezvous,
}

struct PreferredRoute<'a> {
    target: &'a db::ModelRouteCandidateTarget,
    reason: PreferredRouteReason,
}

pub(in crate::worker::runtime) struct SelectedRoute {
    pub(in crate::worker::runtime) route: db::RouteConfig,
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

pub(in crate::worker::runtime) async fn select_route_for_candidate(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    candidate: &db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    user_id: i64,
    routing_key: Option<&str>,
) -> anyhow::Result<Option<SelectedRoute>> {
    if candidate.routing_strategy == db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
        && request.path == "/v1/responses"
    {
        let selected =
            super::session_affinity::select(services, request_ctx, candidate, request, user_id)
                .await?;
        return Ok(Some(SelectedRoute {
            route: route_from_target(
                selected.target,
                user_id,
                candidate.rule_id,
                selected.key_selection.selection,
                selected.route_selection_reason,
            ),
        }));
    }

    let preferred = preferred_target(candidate, &request_ctx.request_prompt_log, routing_key);
    let Some(preferred) = preferred else {
        return Ok(None);
    };
    let route_selection_reason = match preferred.reason {
        PreferredRouteReason::ConversationOverride => {
            db::RouteSelectionReason::ConversationOverride
        }
        PreferredRouteReason::Rendezvous => db::RouteSelectionReason::Default,
    };
    let target = preferred.target;
    let key_selection = select_endpoint_api_key(target, request, &request_ctx.request_prompt_log);
    clear_invalid_conversation_endpoint_key_override(
        services,
        &request_ctx.request_prompt_log,
        key_selection.invalid_conversation_override,
    )
    .await;
    Ok(Some(SelectedRoute {
        route: route_from_target(
            target,
            user_id,
            candidate.rule_id,
            key_selection.selection,
            route_selection_reason,
        ),
    }))
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
        responses_continuation_policy: target.responses_continuation_policy,
        route_selection_reason,
    }
}

fn preferred_target<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
    routing_key: Option<&str>,
) -> Option<PreferredRoute<'a>> {
    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id
        && let Some(target) = candidate
            .targets
            .iter()
            .find(|target| target.endpoint_id == endpoint_id)
    {
        return Some(PreferredRoute {
            target,
            reason: PreferredRouteReason::ConversationOverride,
        });
    }
    rendezvous_target(candidate, routing_key).map(|target| PreferredRoute {
        target,
        reason: PreferredRouteReason::Rendezvous,
    })
}

pub(super) fn candidate_target_by_endpoint(
    candidate: &db::ModelRouteCandidate,
    endpoint_id: uuid::Uuid,
) -> Option<&db::ModelRouteCandidateTarget> {
    candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id == endpoint_id)
}

pub(in crate::worker::runtime) fn rendezvous_target<'a>(
    candidate: &'a db::ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Option<&'a db::ModelRouteCandidateTarget> {
    crate::routing::rendezvous_target(candidate, routing_key)
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

pub(in crate::worker::runtime) fn materialize_route_api_key_selection(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> EndpointApiKeySelectionResult {
    select_api_key(
        route.route_id,
        &route.api_key,
        &route.api_keys,
        route.key_lb_enabled,
        request,
        request_prompt_log,
    )
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

pub(super) fn select_endpoint_api_key(
    target: &db::ModelRouteCandidateTarget,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> EndpointApiKeySelectionResult {
    select_api_key(
        target.endpoint_id,
        &target.api_key,
        &target.api_keys,
        target.key_lb_enabled,
        request,
        request_prompt_log,
    )
}

pub(in crate::worker::runtime) struct EndpointApiKeySelectionResult {
    pub(in crate::worker::runtime) selection: db::EndpointApiKeySelection,
    pub(in crate::worker::runtime) invalid_conversation_override: bool,
}

pub(super) fn select_bound_api_key(
    target: &db::ModelRouteCandidateTarget,
    binding: &ResponseAffinityBinding,
) -> Option<db::EndpointApiKeySelection> {
    let available_keys = target
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == target.endpoint_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    let selected = if let Some(key_id) = binding.endpoint_key_id {
        available_keys.into_iter().find(|key| key.key_id == key_id)
    } else {
        available_keys
            .into_iter()
            .find(|key| api_key_fingerprint(&key.api_key) == binding.endpoint_key_fingerprint)
    };
    selected
        .map(|key| db::EndpointApiKeySelection {
            key_id: (!key.key_id.is_nil()).then_some(key.key_id),
            key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
            secret: key.api_key.clone(),
        })
        .or_else(|| {
            (binding.endpoint_key_id.is_none()
                && api_key_fingerprint(&target.api_key) == binding.endpoint_key_fingerprint)
                .then(|| db::EndpointApiKeySelection {
                    key_id: None,
                    key_label: None,
                    secret: target.api_key.clone(),
                })
        })
}

fn select_api_key(
    endpoint_id: uuid::Uuid,
    fallback_secret: &str,
    api_keys: &[db::EndpointApiKey],
    key_lb_enabled: bool,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> EndpointApiKeySelectionResult {
    let mut available_keys = api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == endpoint_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    if let Some(override_key_id) = request_prompt_log.conversation_override_endpoint_key_id
        && let Some(key) = available_keys
            .iter()
            .find(|key| key.key_id == override_key_id && key.endpoint_id == endpoint_id)
    {
        return EndpointApiKeySelectionResult {
            selection: db::EndpointApiKeySelection {
                key_id: (!key.key_id.is_nil()).then_some(key.key_id),
                key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
                secret: key.api_key.clone(),
            },
            invalid_conversation_override: false,
        };
    }
    available_keys.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.key_label.cmp(&right.key_label))
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    let selected = if key_lb_enabled {
        endpoint_key_stickiness_value(request, request_prompt_log).and_then(|stable_key| {
            stable_candidate_order(
                &available_keys,
                |_, key| stable_endpoint_api_key_score(&stable_key, key),
                |left_index, left, right_index, right| {
                    left.position
                        .cmp(&right.position)
                        .then_with(|| left.key_label.cmp(&right.key_label))
                        .then_with(|| left_index.cmp(&right_index))
                },
            )
            .into_iter()
            .next()
            .and_then(|index| available_keys.get(index).copied())
        })
    } else {
        None
    }
    .or_else(|| available_keys.first().copied());
    let selection = selected
        .map(|key| db::EndpointApiKeySelection {
            key_id: (!key.key_id.is_nil()).then_some(key.key_id),
            key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
            secret: key.api_key.clone(),
        })
        .unwrap_or_else(|| db::EndpointApiKeySelection {
            key_id: None,
            key_label: None,
            secret: fallback_secret.to_string(),
        });
    EndpointApiKeySelectionResult {
        selection,
        invalid_conversation_override: request_prompt_log
            .conversation_override_endpoint_key_id
            .is_some(),
    }
}

fn stable_endpoint_api_key_score(stable_key: &str, key: &db::EndpointApiKey) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(stable_key.as_bytes());
    hasher.update(key.endpoint_id.as_bytes());
    hasher.update(key.key_id.as_bytes());
    if key.key_id.is_nil() {
        hasher.update(key.key_label.as_bytes());
    }
    hasher.finalize().into()
}
