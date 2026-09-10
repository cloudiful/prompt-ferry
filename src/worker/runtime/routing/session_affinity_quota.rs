use crate::{
    db,
    response_affinity::{ResponseAffinityBinding, api_key_fingerprint},
    routing::{candidate_target_by_endpoint, select_bound_api_key},
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};

use super::{quota_selection::request_model, session_affinity::SessionAffinitySelection};

/// Outcome of resolving a session-affinity binding against the current
/// candidate and quota snapshot.
pub(super) enum BindingSelection<'a> {
    /// The bound endpoint/key is usable as-is.
    Selected(SessionAffinitySelection<'a>),
    /// The bound key still exists but its quota window is exhausted; the
    /// caller may migrate to another key/endpoint in the same candidate.
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

/// Whether a candidate target still has at least one usable API key.
pub(super) fn target_has_quota(
    target: &db::ModelRouteCandidateTarget,
    cache: &TokenPlanQuotaCache,
    model: Option<&str>,
) -> bool {
    let mut has_keys = false;
    for key in &target.api_keys {
        if key.endpoint_id != target.endpoint_id || !key.enabled || key.api_key.trim().is_empty() {
            continue;
        }
        has_keys = true;
        if !key_quota_exhausted(cache, target.endpoint_id, key.key_id, model) {
            return true;
        }
    }
    // No per-key rows means the endpoint falls back to its single secret,
    // which carries no quota signal and stays eligible.
    !has_keys
}

/// Narrow a candidate to the targets that still have quota. Returns `None`
/// when nothing is filtered out (or every target is exhausted) so callers
/// keep their existing fallback behavior instead of dropping the route.
pub(super) fn quota_scoped_candidate(
    candidate: &db::ModelRouteCandidate,
    cache: &TokenPlanQuotaCache,
    model: Option<&str>,
) -> Option<db::ModelRouteCandidate> {
    let targets = candidate
        .targets
        .iter()
        .filter(|target| target_has_quota(target, cache, model))
        .cloned()
        .collect::<Vec<_>>();
    if targets.is_empty() || targets.len() == candidate.targets.len() {
        return None;
    }
    Some(db::ModelRouteCandidate {
        targets,
        ..candidate.clone()
    })
}

/// First key in deterministic position order that still has quota,
/// optionally skipping the exhausted bound key.
pub(super) fn usable_target_key(
    target: &db::ModelRouteCandidateTarget,
    cache: &TokenPlanQuotaCache,
    model: Option<&str>,
    exclude: Option<&ResponseAffinityBinding>,
) -> Option<db::EndpointApiKeySelection> {
    let mut keys = target
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == target.endpoint_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    keys.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.key_label.cmp(&right.key_label))
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    if keys.is_empty() {
        return (!target.api_key.trim().is_empty()).then(|| db::EndpointApiKeySelection {
            key_id: None,
            key_label: None,
            secret: target.api_key.clone(),
        });
    }
    keys.into_iter()
        .find(|key| {
            if let Some(binding) = exclude {
                if binding.endpoint_key_id == Some(key.key_id) {
                    return false;
                }
                if binding.endpoint_key_id.is_none()
                    && api_key_fingerprint(&key.api_key) == binding.endpoint_key_fingerprint
                {
                    return false;
                }
            }
            !key_quota_exhausted(cache, target.endpoint_id, key.key_id, model)
        })
        .map(|key| db::EndpointApiKeySelection {
            key_id: (!key.key_id.is_nil()).then_some(key.key_id),
            key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
            secret: key.api_key.clone(),
        })
}

fn previous_response_chain(request_prompt_log: &RequestPromptLog) -> bool {
    request_prompt_log
        .request_previous_response_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

/// Pick a replacement binding once the bound key is quota-exhausted.
///
/// The key on the bound endpoint is rotated first so the upstream node
/// (and any provider-side `previous_response_id` chain) stays put. Only
/// when that is impossible and the request is not a `previous_response_id`
/// continuation do we migrate to another target in the same candidate.
pub(super) fn quota_failover_selection<'a>(
    candidate: &'a db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: &TokenPlanQuotaCache,
) -> Option<(SessionAffinitySelection<'a>, ResponseAffinityBinding)> {
    let model = request_model(request);
    let model = model.as_deref();

    if let Some(target) =
        candidate_target_by_endpoint(candidate, binding.endpoint_id).filter(|target| target.enabled)
        && let Some(key_selection) = usable_target_key(target, quota_cache, model, Some(binding))
    {
        return Some((
            SessionAffinitySelection {
                target,
                key_selection: key_selection.clone(),
                route_selection_reason: db::RouteSelectionReason::QuotaFailover,
            },
            binding_for_selection(target, &key_selection),
        ));
    }

    if previous_response_chain(request_prompt_log) {
        return None;
    }

    for target in &candidate.targets {
        if target.endpoint_id == binding.endpoint_id || !target.enabled {
            continue;
        }
        if !target_has_quota(target, quota_cache, model) {
            continue;
        }
        if let Some(key_selection) = usable_target_key(target, quota_cache, model, None) {
            return Some((
                SessionAffinitySelection {
                    target,
                    key_selection: key_selection.clone(),
                    route_selection_reason: db::RouteSelectionReason::QuotaFailover,
                },
                binding_for_selection(target, &key_selection),
            ));
        }
    }

    None
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
