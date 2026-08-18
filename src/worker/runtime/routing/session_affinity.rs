use std::fmt;

use anyhow::Result;
use http::StatusCode;

use crate::{
    db,
    response_affinity::{
        ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint, log_unavailable,
    },
    worker::runtime::context::AffinityFailureAudit,
};

use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use super::selection::{endpoint_key_stickiness_value, rendezvous_target, select_endpoint_api_key};
use super::session_affinity_quota::{
    binding_for_selection, bound_key_exhausted, selection_for_binding,
};
use crate::routing::candidate_target_by_endpoint;

#[derive(Debug, Clone)]
pub(in crate::worker::runtime) struct RouteAffinityError {
    pub(in crate::worker::runtime) status: StatusCode,
    pub(in crate::worker::runtime) code: &'static str,
    pub(in crate::worker::runtime) message: &'static str,
    pub(in crate::worker::runtime) audit: AffinityFailureAudit,
}

impl RouteAffinityError {
    fn identity_required() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "responses_session_identity_required",
            message: "session affinity requires a stable session identity",
            audit: AffinityFailureAudit::default(),
        }
    }

    fn backend_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "responses_session_affinity_unavailable",
            message: "session affinity backend is unavailable",
            audit: AffinityFailureAudit::default(),
        }
    }

    fn target_unavailable(audit: AffinityFailureAudit) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "responses_session_affinity_target_unavailable",
            message: "the bound session endpoint or API key is unavailable",
            audit,
        }
    }

    fn conflict(audit: AffinityFailureAudit) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "responses_session_affinity_conflict",
            message: "the requested endpoint or API key conflicts with the bound session",
            audit,
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
    for target in &candidate.targets {
        if let Err(err) = admin_state
            .token_plan_quota
            .refresh_if_due(&admin_state.pool, target.endpoint_id)
            .await
        {
            tracing::warn!(
                endpoint_id = %target.endpoint_id,
                error = %err,
                "MiniMax quota refresh failed during session-affinity selection"
            );
        }
    }

    for _ in 0..2 {
        if let Some(current_binding) = binding.as_ref() {
            let audit = binding_audit(candidate.rule_id, Some(current_binding), request_prompt_log);
            if override_conflicts_with_binding(current_binding, request_prompt_log) {
                let rebindable =
                    override_rebind_target(candidate, request_prompt_log).is_some_and(|target| {
                        target.responses_continuation_policy
                            == db::ResponsesContinuationPolicy::ForceReplay
                    });
                if !rebindable {
                    return Err(anyhow::Error::new(RouteAffinityError::conflict(audit)));
                }
                let (selection, replacement) = select_new_binding(
                    candidate,
                    request,
                    request_prompt_log,
                    &stable_identity,
                    &audit,
                    &admin_state.token_plan_quota,
                )?;
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
            if let Some(selection) = selection_for_binding(
                candidate,
                current_binding,
                request,
                Some(&admin_state.token_plan_quota),
            ) {
                heal_stale_binding(&store, &cache_key, current_binding, &selection).await;
                return Ok(selection);
            }
            if let Some(target) =
                candidate_target_by_endpoint(candidate, current_binding.endpoint_id)
                    .filter(|target| target.enabled)
            {
                let quota_exhausted = bound_key_exhausted(
                    candidate,
                    current_binding,
                    request,
                    Some(&admin_state.token_plan_quota),
                );
                if !quota_exhausted
                    || target.responses_continuation_policy
                        != db::ResponsesContinuationPolicy::ForceReplay
                {
                    return Err(anyhow::Error::new(RouteAffinityError::target_unavailable(
                        audit,
                    )));
                }
            }

            let (selection, replacement) = select_new_binding(
                candidate,
                request,
                request_prompt_log,
                &stable_identity,
                &audit,
                &admin_state.token_plan_quota,
            )?;
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

        let (selection, candidate_binding) = select_new_binding(
            candidate,
            request,
            request_prompt_log,
            &stable_identity,
            &AffinityFailureAudit::default(),
            &admin_state.token_plan_quota,
        )?;
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

    Err(anyhow::Error::new(RouteAffinityError::target_unavailable(
        binding_audit(candidate.rule_id, binding.as_ref(), request_prompt_log),
    )))
}

fn binding_audit(
    rule_id: uuid::Uuid,
    binding: Option<&ResponseAffinityBinding>,
    request_prompt_log: &RequestPromptLog,
) -> AffinityFailureAudit {
    AffinityFailureAudit {
        model_route_rule_id: Some(rule_id),
        endpoint_id: binding.map(|entry| entry.endpoint_id),
        endpoint_key_id: binding.and_then(|entry| entry.endpoint_key_id),
        requested_endpoint_id: request_prompt_log.conversation_override_endpoint_id,
        requested_key_id: request_prompt_log.conversation_override_endpoint_key_id,
    }
}

fn override_rebind_target<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
) -> Option<&'a db::ModelRouteCandidateTarget> {
    request_prompt_log
        .conversation_override_endpoint_id
        .and_then(|endpoint_id| candidate_target_by_endpoint(candidate, endpoint_id))
        .filter(|target| target.enabled)
}

async fn heal_stale_binding(
    store: &ResponseAffinityStore,
    cache_key: &str,
    binding: &ResponseAffinityBinding,
    selection: &SessionAffinitySelection<'_>,
) {
    let replacement = ResponseAffinityBinding {
        endpoint_id: selection.target.endpoint_id,
        endpoint_key_id: selection.key_selection.key_id,
        endpoint_key_fingerprint: api_key_fingerprint(&selection.key_selection.secret),
    };
    if replacement == *binding {
        return;
    }
    if let Err(err) = store
        .replace_if_current(cache_key, binding, &replacement)
        .await
    {
        log_unavailable(&err);
    }
}

fn session_target_for_new_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
    audit: &AffinityFailureAudit,
) -> Result<(&'a db::ModelRouteCandidateTarget, db::RouteSelectionReason)> {
    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| {
                anyhow::Error::new(RouteAffinityError::target_unavailable(audit.clone()))
            })?;
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
        .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable(audit.clone())))
}

fn select_new_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
    audit: &AffinityFailureAudit,
    quota_cache: &crate::worker_admin::token_plan_cache::TokenPlanQuotaCache,
) -> Result<(SessionAffinitySelection<'a>, ResponseAffinityBinding)> {
    let (target, route_selection_reason) =
        session_target_for_new_binding(candidate, request_prompt_log, stable_identity, audit)?;
    let key_selection =
        select_endpoint_api_key(target, request, request_prompt_log, Some(quota_cache));
    if key_selection.invalid_conversation_override {
        return Err(anyhow::Error::new(RouteAffinityError::conflict(
            audit.clone(),
        )));
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

fn override_conflicts_with_binding(
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
