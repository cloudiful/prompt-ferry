use crate::{
    db,
    response_affinity::{ResponseAffinityBinding, api_key_fingerprint},
    routing::{candidate_target_by_endpoint, select_bound_api_key},
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};

use super::{key_pool, quota_selection::request_model, session_affinity::SessionAffinitySelection};

/// Outcome of resolving a session-affinity binding against the current
/// candidate and quota snapshot.
pub(super) enum BindingSelection<'a> {
    /// The bound endpoint/key is usable as-is.
    Selected(SessionAffinitySelection<'a>),
    /// The bound key still exists but its quota window is exhausted; the
    /// caller redraws from the unified pool with that unit removed.
    QuotaExhausted,
    /// The bound endpoint or key is gone/disabled; strict affinity applies.
    Unavailable,
}

pub(super) fn selection_for_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
    request: &BufferedBridgeRequest,
    quota_cache: Option<&TokenPlanQuotaCache>,
) -> BindingSelection<'a> {
    let Some(target) = candidate_target_by_endpoint(candidate, binding.endpoint_id)
        .filter(|target| target.enabled)
    else {
        return BindingSelection::Unavailable;
    };
    let Some(key_selection) = select_bound_api_key(target, binding) else {
        return BindingSelection::Unavailable;
    };
    if let (Some(quota_cache), Some(key_id)) = (quota_cache, key_selection.key_id)
        && key_quota_exhausted(
            quota_cache,
            target.endpoint_id,
            key_id,
            request_model(request).as_deref(),
        )
    {
        return BindingSelection::QuotaExhausted;
    }
    BindingSelection::Selected(SessionAffinitySelection {
        target,
        key_selection,
        route_selection_reason: db::RouteSelectionReason::SessionAffinity,
    })
}

pub(super) fn binding_for_selection(
    target: &db::ModelRouteCandidateTarget,
    key_selection: &db::EndpointApiKeySelection,
) -> ResponseAffinityBinding {
    ResponseAffinityBinding {
        endpoint_id: target.endpoint_id,
        endpoint_key_id: key_selection.key_id,
        endpoint_key_fingerprint: api_key_fingerprint(&key_selection.secret),
    }
}

/// A key is exhausted when the quota cache has a known window and its
/// remaining percentage has bottomed out. Keys without a quota signal
/// (PAYG, unsupported providers) are never treated as exhausted.
pub(super) fn key_quota_exhausted(
    cache: &TokenPlanQuotaCache,
    endpoint_id: uuid::Uuid,
    key_id: uuid::Uuid,
    model: Option<&str>,
) -> bool {
    cache
        .key_remaining_percent_now(endpoint_id, key_id, model)
        .is_some_and(|remaining| remaining <= 0.0)
}

fn previous_response_chain(request_prompt_log: &RequestPromptLog) -> bool {
    request_prompt_log
        .request_previous_response_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

/// Unit id of the currently bound selection so the redraw can exclude it.
/// Keys use their key id; a target-level secret unit uses the target id.
///
/// Legacy bindings predate `endpoint_key_id` and carry only the endpoint plus
/// a key fingerprint. Resolve the fingerprint back to the current key row so
/// the exclusion matches the pool unit id (`key_id`) instead of falling back
/// to the incomparable `target_id`.
fn bound_unit_id(
    candidate: &db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
) -> Option<uuid::Uuid> {
    if let Some(key_id) = binding.endpoint_key_id {
        return Some(key_id);
    }
    candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id == binding.endpoint_id)
        .and_then(|target| select_bound_api_key(target, binding))
        .and_then(|selection| selection.key_id)
}

/// Redraw after the bound key's quota is exhausted. The bound unit is
/// removed from the pool and a fresh weighted unit is drawn. A
/// `previous_response_id` continuation keeps the upstream node pinned: the
/// pool is narrowed to the bound endpoint and no cross-endpoint migration
/// is allowed.
pub(super) fn quota_failover_selection<'a>(
    candidate: &'a db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: &TokenPlanQuotaCache,
    stable_key: &str,
    estimated_tokens: u64,
) -> Option<(SessionAffinitySelection<'a>, ResponseAffinityBinding)> {
    let model = request_model(request);
    let model = model.as_deref();
    let exclude = bound_unit_id(candidate, binding);
    let chained = previous_response_chain(request_prompt_log);
    let mut units =
        key_pool::candidate_units_all_keys(candidate, Some(quota_cache), model, exclude);
    if units.is_empty() {
        units = key_pool::candidate_units_without_quota_all_keys(candidate, exclude);
    }
    if chained {
        units.retain(|unit| unit.target.endpoint_id == binding.endpoint_id);
    }
    let unit = key_pool::draw(&units, stable_key, Some(quota_cache), estimated_tokens)?;
    Some((
        SessionAffinitySelection {
            target: unit.target,
            key_selection: unit.key.clone(),
            route_selection_reason: db::RouteSelectionReason::QuotaFailover,
        },
        binding_for_selection(unit.target, &unit.key),
    ))
}

pub(super) fn log_quota_failover(
    rule_id: uuid::Uuid,
    from: &ResponseAffinityBinding,
    to: &ResponseAffinityBinding,
    request_prompt_log: &RequestPromptLog,
) {
    tracing::warn!(
        event = "quota_failover",
        model_route_rule_id = %rule_id,
        from_endpoint_id = %from.endpoint_id,
        from_endpoint_key_id = from.endpoint_key_id.map(|id| id.to_string()).unwrap_or_default(),
        to_endpoint_id = %to.endpoint_id,
        to_endpoint_key_id = to.endpoint_key_id.map(|id| id.to_string()).unwrap_or_default(),
        endpoint_changed = from.endpoint_id != to.endpoint_id,
        previous_response_chain = previous_response_chain(request_prompt_log),
        "session affinity migrated after bound key quota exhaustion"
    );
}
