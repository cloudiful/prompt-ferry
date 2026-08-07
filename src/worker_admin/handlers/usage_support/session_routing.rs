use super::*;
use crate::response_affinity::{ResponseAffinityBinding, ResponseAffinityStore};
use crate::routing::{BoundBindingState, bound_binding_state, candidate_target_by_endpoint};

pub(in crate::worker_admin::handlers) async fn build_session_route_options_response(
    state: &AdminState,
    record_id: i64,
    visible_user_id: Option<i64>,
    fallback_user_id: i64,
) -> Result<SessionRouteOptionsResponse, Response> {
    let event =
        get_visible_usage_event_detail_or_not_found(state, record_id, visible_user_id).await?;
    let Some(conversation_id) = event.conversation_id else {
        return Err(bad_request("request record has no conversation_id"));
    };
    let route_user_id = event.user_id.unwrap_or(fallback_user_id);
    let record_rule_id = db::get_request_record_route_locator(&state.pool, record_id)
        .await
        .map_err(|err| internal(state, err))?
        .and_then(|locator| locator.model_route_rule_id);
    let override_entry = db::get_conversation_endpoint_override(&state.pool, conversation_id)
        .await
        .map_err(|err| internal(state, err))?;
    let (fallback_route, candidate) = db::resolve_model_route_with_fallback(
        &state.pool,
        route_user_id,
        event.model.as_deref(),
        true,
    )
    .await
    .map_err(|err| internal(state, err))?;

    let mut options = if let Some(candidate) = candidate.as_ref() {
        build_candidate_session_route_options(&event, &override_entry, candidate)
    } else {
        Vec::new()
    };
    if options.is_empty()
        && let Some(endpoint_id) = event
            .endpoint_id
            .or(fallback_route.map(|route| route.route_id))
        && let Ok(Some(endpoint)) = db::get_endpoint(&state.pool, endpoint_id).await
    {
        options.push(db::SessionRouteOption {
            endpoint_id,
            endpoint_name: endpoint.name,
            keys: endpoint
                .api_keys
                .iter()
                .filter(|key| key.enabled && !key.key_id.is_nil())
                .map(|key| db::SessionRouteKeyOption {
                    key_id: key.key_id,
                    key_label: key.key_label.clone(),
                })
                .collect(),
            is_override: override_entry
                .as_ref()
                .is_some_and(|entry| entry.endpoint_id == endpoint_id),
            is_preferred: true,
        });
    }

    let affinity = resolve_session_affinity_status(
        &state.replay_cache.response_affinity(),
        route_user_id,
        conversation_id,
        candidate.as_ref(),
        record_rule_id,
    )
    .await;

    Ok(SessionRouteOptionsResponse {
        conversation_id,
        current_endpoint_id: event.endpoint_id,
        current_endpoint_key_id: event.endpoint_key_id,
        current_endpoint_key_label: event.endpoint_key_label.clone(),
        override_endpoint_id: override_entry.as_ref().map(|entry| entry.endpoint_id),
        override_endpoint_key_id: override_entry
            .as_ref()
            .and_then(|entry| entry.endpoint_key_id),
        override_endpoint_key_label: override_entry
            .as_ref()
            .and_then(|entry| entry.endpoint_key_label.clone()),
        options,
        affinity,
    })
}

fn build_candidate_session_route_options(
    event: &db::UsageEventDetail,
    override_entry: &Option<db::ConversationEndpointOverride>,
    candidate: &db::ModelRouteCandidate,
) -> Vec<db::SessionRouteOption> {
    candidate
        .targets
        .iter()
        .map(|target| db::SessionRouteOption {
            endpoint_id: target.endpoint_id,
            endpoint_name: target.endpoint_name.clone(),
            keys: target
                .api_keys
                .iter()
                .filter(|key| key.enabled && !key.key_id.is_nil())
                .map(|key| db::SessionRouteKeyOption {
                    key_id: key.key_id,
                    key_label: key.key_label.clone(),
                })
                .collect(),
            is_override: override_entry
                .as_ref()
                .is_some_and(|entry| entry.endpoint_id == target.endpoint_id),
            is_preferred: event.endpoint_id == Some(target.endpoint_id),
        })
        .collect()
}

async fn resolve_session_affinity_status(
    store: &ResponseAffinityStore,
    route_user_id: i64,
    conversation_id: uuid::Uuid,
    candidate: Option<&db::ModelRouteCandidate>,
    record_rule_id: Option<uuid::Uuid>,
) -> SessionAffinityStatus {
    let stable_identity = format!("conversation:{conversation_id}");
    let resolved_rule_id = candidate.map(|candidate| candidate.rule_id);

    let (mut binding_rule_id, mut binding) = (None, None);
    if let Some(candidate) = candidate {
        binding_rule_id = Some(candidate.rule_id);
        binding = peek_binding(store, route_user_id, candidate.rule_id, &stable_identity).await;
        if binding.is_none()
            && record_rule_id.is_some_and(|rule_id| rule_id != candidate.rule_id)
            && let Some(rule_id) = record_rule_id
        {
            binding_rule_id = Some(rule_id);
            binding = peek_binding(store, route_user_id, rule_id, &stable_identity).await;
        }
    }

    let Some(binding) = binding else {
        return SessionAffinityStatus {
            state: SessionAffinityState::Unbound,
            rule_id: resolved_rule_id,
            endpoint_id: None,
            endpoint_name: None,
            key_id: None,
            key_label: None,
        };
    };
    let status = affinity_status_for_binding(candidate, binding_rule_id, &binding);
    status
}

fn affinity_status_for_binding(
    candidate: Option<&db::ModelRouteCandidate>,
    rule_id: Option<uuid::Uuid>,
    binding: &ResponseAffinityBinding,
) -> SessionAffinityStatus {
    let Some(candidate) = candidate else {
        return SessionAffinityStatus {
            state: SessionAffinityState::StaleEndpoint,
            rule_id,
            endpoint_id: Some(binding.endpoint_id),
            endpoint_name: None,
            key_id: binding.endpoint_key_id,
            key_label: None,
        };
    };
    let state = match bound_binding_state(candidate, binding) {
        BoundBindingState::Active => SessionAffinityState::Active,
        BoundBindingState::StaleEndpoint => SessionAffinityState::StaleEndpoint,
        BoundBindingState::StaleKey => SessionAffinityState::StaleKey,
    };
    let Some(target) = candidate_target_by_endpoint(candidate, binding.endpoint_id) else {
        return SessionAffinityStatus {
            state,
            rule_id,
            endpoint_id: Some(binding.endpoint_id),
            endpoint_name: None,
            key_id: binding.endpoint_key_id,
            key_label: None,
        };
    };
    let key_selection = crate::routing::select_bound_api_key(target, binding);
    SessionAffinityStatus {
        state,
        rule_id,
        endpoint_id: Some(target.endpoint_id),
        endpoint_name: Some(target.endpoint_name.clone()),
        key_id: key_selection
            .as_ref()
            .and_then(|selection| selection.key_id)
            .or(binding.endpoint_key_id),
        key_label: key_selection
            .as_ref()
            .and_then(|selection| selection.key_label.clone()),
    }
}

async fn peek_binding(
    store: &ResponseAffinityStore,
    route_user_id: i64,
    rule_id: uuid::Uuid,
    stable_identity: &str,
) -> Option<ResponseAffinityBinding> {
    let cache_key = ResponseAffinityStore::cache_key(route_user_id, rule_id, stable_identity);
    match store.peek(&cache_key).await {
        Ok(binding) => binding,
        Err(err) => {
            tracing::warn!(error = %err, "failed to peek session affinity binding");
            None
        }
    }
}
