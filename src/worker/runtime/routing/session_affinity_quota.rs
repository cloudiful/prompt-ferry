use crate::{
    db,
    response_affinity::{ResponseAffinityBinding, api_key_fingerprint},
    routing::{candidate_target_by_endpoint, select_bound_api_key},
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};

use super::{quota_selection::request_model, session_affinity::SessionAffinitySelection};

pub(super) fn selection_for_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
    request: &BufferedBridgeRequest,
    quota_cache: Option<&TokenPlanQuotaCache>,
) -> Option<SessionAffinitySelection<'a>> {
    let target = candidate_target_by_endpoint(candidate, binding.endpoint_id)
        .filter(|target| target.enabled)?;
    let key_selection = select_bound_api_key(target, binding)?;
    if let (Some(quota_cache), Some(key_id)) = (quota_cache, key_selection.key_id)
        && quota_cache
            .key_remaining_percent_now(
                target.endpoint_id,
                key_id,
                request_model(request).as_deref(),
            )
            .is_some_and(|remaining| remaining <= 0.0)
    {
        return None;
    }
    Some(SessionAffinitySelection {
        target,
        key_selection,
        route_selection_reason: db::RouteSelectionReason::SessionAffinity,
    })
}

pub(super) fn bound_key_exhausted(
    candidate: &db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
    request: &BufferedBridgeRequest,
    quota_cache: Option<&TokenPlanQuotaCache>,
) -> bool {
    let Some(quota_cache) = quota_cache else {
        return false;
    };
    let Some(target) = candidate_target_by_endpoint(candidate, binding.endpoint_id) else {
        return false;
    };
    let Some(key_selection) = select_bound_api_key(target, binding) else {
        return false;
    };
    let Some(key_id) = key_selection.key_id else {
        return false;
    };
    quota_cache
        .key_remaining_percent_now(
            target.endpoint_id,
            key_id,
            request_model(request).as_deref(),
        )
        .is_some_and(|remaining| remaining <= 0.0)
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
