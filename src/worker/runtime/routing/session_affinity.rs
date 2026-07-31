use std::fmt;

use anyhow::Result;
use http::StatusCode;

use crate::{
    db,
    response_affinity::{
        ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint, log_unavailable,
    },
};

use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use super::selection::{
    EndpointApiKeySelectionResult, candidate_target_by_endpoint, endpoint_key_stickiness_value,
    rendezvous_target, select_bound_api_key, select_endpoint_api_key,
};

#[derive(Debug)]
pub(in crate::worker::runtime) struct RouteAffinityError {
    pub(in crate::worker::runtime) status: StatusCode,
    pub(in crate::worker::runtime) code: &'static str,
    pub(in crate::worker::runtime) message: &'static str,
}

impl RouteAffinityError {
    fn identity_required() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "responses_session_identity_required",
            message: "responses session affinity requires a stable session identity",
        }
    }

    fn backend_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "responses_session_affinity_unavailable",
            message: "responses session affinity backend is unavailable",
        }
    }

    fn target_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "responses_session_affinity_target_unavailable",
            message: "the bound responses session endpoint or API key is unavailable",
        }
    }

    fn conflict() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "responses_session_affinity_conflict",
            message: "the requested endpoint or API key conflicts with the bound responses session",
        }
    }
}

impl fmt::Display for RouteAffinityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for RouteAffinityError {}

pub(super) struct SessionAffinitySelection<'a> {
    pub(super) target: &'a db::ModelRouteCandidateTarget,
    pub(super) key_selection: EndpointApiKeySelectionResult,
    pub(super) route_selection_reason: db::RouteSelectionReason,
}

pub(super) async fn select<'a>(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    candidate: &'a db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    user_id: i64,
) -> Result<SessionAffinitySelection<'a>> {
    let request_prompt_log = &request_ctx.request_prompt_log;
    let Some(stable_identity) = endpoint_key_stickiness_value(request, request_prompt_log) else {
        return Err(anyhow::Error::new(RouteAffinityError::identity_required()));
    };

    let Some(admin_state) = services.admin_state() else {
        return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
    };

    let store = admin_state.replay_cache.response_affinity();
    let cache_key = ResponseAffinityStore::cache_key(user_id, candidate.rule_id, &stable_identity);
    let existing_binding = match store.get(&cache_key).await {
        Ok(binding) => binding,
        Err(err) => {
            log_unavailable(&err);
            return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
        }
    };

    let (target, initial_key_selection, route_selection_reason) = match existing_binding.as_ref() {
        Some(binding) => {
            if binding_conflicts_with_override(binding, request_prompt_log) {
                return Err(anyhow::Error::new(RouteAffinityError::conflict()));
            }
            let target = candidate_target_by_endpoint(candidate, binding.endpoint_id)
                .filter(|target| target.enabled)
                .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
            let key_selection = select_bound_api_key(target, binding)
                .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
            (
                target,
                EndpointApiKeySelectionResult {
                    selection: key_selection,
                    invalid_conversation_override: false,
                },
                db::RouteSelectionReason::SessionAffinity,
            )
        }
        None => {
            let (target, reason) = initial_session_target(candidate, request_prompt_log)?
                .or_else(|| {
                    rendezvous_target(candidate, Some(&stable_identity))
                        .map(|target| (target, db::RouteSelectionReason::SessionAffinity))
                })
                .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
            let key_selection = select_endpoint_api_key(target, request, request_prompt_log);
            if key_selection.invalid_conversation_override {
                return Err(anyhow::Error::new(RouteAffinityError::conflict()));
            }
            (target, key_selection, reason)
        }
    };

    let candidate_binding = binding_for_selection(target, &initial_key_selection);
    let binding = if existing_binding.is_some() {
        existing_binding.expect("checked above")
    } else {
        match store.get_or_create(&cache_key, &candidate_binding).await {
            Ok(binding) => binding,
            Err(err) => {
                log_unavailable(&err);
                return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
            }
        }
    };

    if binding_conflicts_with_override(&binding, request_prompt_log) {
        return Err(anyhow::Error::new(RouteAffinityError::conflict()));
    }

    if binding.endpoint_id != target.endpoint_id
        || !binding_matches_selection(&binding, &initial_key_selection)
    {
        let target = candidate_target_by_endpoint(candidate, binding.endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
        let key_selection = select_bound_api_key(target, &binding)
            .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
        return Ok(SessionAffinitySelection {
            target,
            key_selection: EndpointApiKeySelectionResult {
                selection: key_selection,
                invalid_conversation_override: false,
            },
            route_selection_reason: db::RouteSelectionReason::SessionAffinity,
        });
    }

    Ok(SessionAffinitySelection {
        target,
        key_selection: initial_key_selection,
        route_selection_reason,
    })
}

fn initial_session_target<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
) -> Result<Option<(&'a db::ModelRouteCandidateTarget, db::RouteSelectionReason)>> {
    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
        return Ok(Some((
            target,
            db::RouteSelectionReason::ConversationOverride,
        )));
    }
    if let Some(endpoint_id) = request_prompt_log.preferred_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
        return Ok(Some((target, db::RouteSelectionReason::SessionAffinity)));
    }
    Ok(None)
}

fn binding_conflicts_with_override(
    binding: &ResponseAffinityBinding,
    request_prompt_log: &RequestPromptLog,
) -> bool {
    request_prompt_log
        .conversation_override_endpoint_id
        .is_some_and(|endpoint_id| endpoint_id != binding.endpoint_id)
        || request_prompt_log
            .conversation_override_endpoint_key_id
            .is_some_and(|key_id| binding.endpoint_key_id != Some(key_id))
}

fn binding_for_selection(
    target: &db::ModelRouteCandidateTarget,
    key_selection: &EndpointApiKeySelectionResult,
) -> ResponseAffinityBinding {
    ResponseAffinityBinding {
        endpoint_id: target.endpoint_id,
        endpoint_key_id: key_selection.selection.key_id,
        endpoint_key_fingerprint: api_key_fingerprint(&key_selection.selection.secret),
    }
}

fn binding_matches_selection(
    binding: &ResponseAffinityBinding,
    key_selection: &EndpointApiKeySelectionResult,
) -> bool {
    match binding.endpoint_key_id {
        Some(key_id) => key_selection.selection.key_id == Some(key_id),
        None => {
            binding.endpoint_key_fingerprint == api_key_fingerprint(&key_selection.selection.secret)
        }
    }
}
