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
    candidate_target_by_endpoint, endpoint_key_stickiness_value, rendezvous_target,
    select_bound_api_key, select_endpoint_api_key,
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
    pub(super) key_selection: db::EndpointApiKeySelection,
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
    let mut binding = match store.get(&cache_key).await {
        Ok(binding) => binding,
        Err(err) => {
            log_unavailable(&err);
            return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
        }
    };

    for _ in 0..2 {
        if let Some(current_binding) = binding.as_ref() {
            if binding_conflicts_with_override(current_binding, request_prompt_log) {
                return Err(anyhow::Error::new(RouteAffinityError::conflict()));
            }
            if let Some(selection) = selection_for_binding(candidate, current_binding) {
                return Ok(selection);
            }
            if candidate_target_by_endpoint(candidate, current_binding.endpoint_id)
                .is_some_and(|target| target.enabled)
            {
                return Err(anyhow::Error::new(RouteAffinityError::target_unavailable()));
            }

            let (selection, replacement) =
                select_new_binding(candidate, request, request_prompt_log, &stable_identity)?;
            match store
                .replace_if_current(&cache_key, current_binding, &replacement)
                .await
            {
                Ok(true) => return Ok(selection),
                Ok(false) => {}
                Err(err) => {
                    log_unavailable(&err);
                    return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
                }
            }
            binding = match store.get(&cache_key).await {
                Ok(binding) => binding,
                Err(err) => {
                    log_unavailable(&err);
                    return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
                }
            };
            continue;
        }

        let (selection, candidate_binding) =
            select_new_binding(candidate, request, request_prompt_log, &stable_identity)?;
        let created = match store.get_or_create(&cache_key, &candidate_binding).await {
            Ok(binding) => binding,
            Err(err) => {
                log_unavailable(&err);
                return Err(anyhow::Error::new(RouteAffinityError::backend_unavailable()));
            }
        };
        if created == candidate_binding {
            return Ok(selection);
        }
        binding = Some(created);
    }

    Err(anyhow::Error::new(RouteAffinityError::target_unavailable()))
}

fn session_target_for_new_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
) -> Result<(&'a db::ModelRouteCandidateTarget, db::RouteSelectionReason)> {
    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))?;
        return Ok((target, db::RouteSelectionReason::ConversationOverride));
    }

    if let Some(target) = request_prompt_log
        .preferred_endpoint_id
        .and_then(|endpoint_id| candidate_target_by_endpoint(candidate, endpoint_id))
        .filter(|target| target.enabled)
    {
        return Ok((target, db::RouteSelectionReason::SessionAffinity));
    }

    rendezvous_target(candidate, Some(stable_identity))
        .map(|target| (target, db::RouteSelectionReason::SessionAffinity))
        .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable()))
}

fn select_new_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
) -> Result<(SessionAffinitySelection<'a>, ResponseAffinityBinding)> {
    let (target, route_selection_reason) =
        session_target_for_new_binding(candidate, request_prompt_log, stable_identity)?;
    let key_selection = select_endpoint_api_key(target, request, request_prompt_log);
    if key_selection.invalid_conversation_override {
        return Err(anyhow::Error::new(RouteAffinityError::conflict()));
    }
    let binding = binding_for_selection(target, &key_selection.selection);
    Ok((
        SessionAffinitySelection {
            target,
            key_selection: key_selection.selection,
            route_selection_reason,
        },
        binding,
    ))
}

fn selection_for_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
) -> Option<SessionAffinitySelection<'a>> {
    let target = candidate_target_by_endpoint(candidate, binding.endpoint_id)
        .filter(|target| target.enabled)?;
    let key_selection = select_bound_api_key(target, binding)?;
    Some(SessionAffinitySelection {
        target,
        key_selection,
        route_selection_reason: db::RouteSelectionReason::SessionAffinity,
    })
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
    key_selection: &db::EndpointApiKeySelection,
) -> ResponseAffinityBinding {
    ResponseAffinityBinding {
        endpoint_id: target.endpoint_id,
        endpoint_key_id: key_selection.key_id,
        endpoint_key_fingerprint: api_key_fingerprint(&key_selection.secret),
    }
}
