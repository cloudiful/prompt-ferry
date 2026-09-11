use std::fmt;

use anyhow::Result;
use http::StatusCode;

use crate::{
    db,
    response_affinity::{
        ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint, log_unavailable,
    },
    routing::candidate_target_by_endpoint,
    worker::runtime::context::AffinityFailureAudit,
    worker_admin::token_plan_cache::{TokenPlanQuotaCache, estimate_input_tokens},
};

use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use super::key_pool;
use super::quota_selection::{refresh_candidate_quota, request_model};
use super::selection::endpoint_key_stickiness_value;
use super::session_affinity_quota::{
    BindingSelection, binding_for_selection, log_quota_failover, quota_failover_selection,
    selection_for_binding,
};

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

struct NewSessionUnit<'a> {
    target: &'a db::ModelRouteCandidateTarget,
    key: db::EndpointApiKeySelection,
    reason: db::RouteSelectionReason,
    invalid_override: bool,
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
    let quota_cache = &admin_state.token_plan_quota;
    refresh_candidate_quota(services, candidate).await;
    let model = request_model(request);
    let estimated = estimate_input_tokens(&request.body);

    for _ in 0..2 {
        if let Some(current_binding) = binding.clone() {
            let audit = binding_audit(
                candidate.rule_id,
                Some(&current_binding),
                request_prompt_log,
            );
            if override_conflicts_with_binding(&current_binding, request_prompt_log) {
                return Err(anyhow::Error::new(RouteAffinityError::conflict(audit)));
            }
            match selection_for_binding(candidate, &current_binding, request, Some(quota_cache)) {
                BindingSelection::Selected(selection) => {
                    heal_stale_binding(&store, &cache_key, &current_binding, &selection).await;
                    return Ok(selection);
                }
                BindingSelection::QuotaExhausted => {
                    let Some((selection, replacement)) = quota_failover_selection(
                        candidate,
                        &current_binding,
                        request,
                        request_prompt_log,
                        quota_cache,
                        &stable_identity,
                        estimated,
                    ) else {
                        return Err(anyhow::Error::new(RouteAffinityError::target_unavailable(
                            audit,
                        )));
                    };
                    match store
                        .replace_if_current(&cache_key, &current_binding, &replacement)
                        .await
                    {
                        Ok(true) => {
                            log_quota_failover(
                                candidate.rule_id,
                                &current_binding,
                                &replacement,
                                request_prompt_log,
                            );
                            return Ok(selection);
                        }
                        Ok(false) => {
                            // A concurrent worker rebound the session first;
                            // reload and re-evaluate on the next pass.
                            binding = match store.get(&cache_key).await {
                                Ok(binding) => binding,
                                Err(err) => {
                                    log_unavailable(&err);
                                    return Err(anyhow::Error::new(
                                        RouteAffinityError::backend_unavailable(),
                                    ));
                                }
                            };
                        }
                        Err(err) => {
                            log_unavailable(&err);
                            return Err(anyhow::Error::new(
                                RouteAffinityError::backend_unavailable(),
                            ));
                        }
                    }
                }
                BindingSelection::Unavailable => {
                    return Err(anyhow::Error::new(RouteAffinityError::target_unavailable(
                        audit,
                    )));
                }
            }
            continue;
        }

        let (selection, candidate_binding) = select_new_binding(
            candidate,
            request_prompt_log,
            &stable_identity,
            &AffinityFailureAudit::default(),
            quota_cache,
            model.as_deref(),
            estimated,
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

fn select_new_binding<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
    audit: &AffinityFailureAudit,
    quota_cache: &TokenPlanQuotaCache,
    model: Option<&str>,
    estimated_tokens: u64,
) -> Result<(SessionAffinitySelection<'a>, ResponseAffinityBinding)> {
    let selected = select_new_session_unit(
        candidate,
        request_prompt_log,
        stable_identity,
        quota_cache,
        model,
        estimated_tokens,
        audit,
    )?;
    if selected.invalid_override {
        return Err(anyhow::Error::new(RouteAffinityError::conflict(
            audit.clone(),
        )));
    }
    let binding = binding_for_selection(selected.target, &selected.key);
    Ok((
        SessionAffinitySelection {
            target: selected.target,
            key_selection: selected.key,
            route_selection_reason: selected.reason,
        },
        binding,
    ))
}

fn select_new_session_unit<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
    quota_cache: &TokenPlanQuotaCache,
    model: Option<&str>,
    estimated_tokens: u64,
    audit: &AffinityFailureAudit,
) -> Result<NewSessionUnit<'a>> {
    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, endpoint_id)
            .filter(|target| target.enabled)
            .ok_or_else(|| {
                anyhow::Error::new(RouteAffinityError::target_unavailable(audit.clone()))
            })?;
        let (key, invalid_override) = select_target_unit(
            target,
            request_prompt_log,
            stable_identity,
            quota_cache,
            model,
            estimated_tokens,
        );
        return Ok(NewSessionUnit {
            target,
            key,
            reason: db::RouteSelectionReason::ConversationOverride,
            invalid_override,
        });
    }

    if let Some(target) = request_prompt_log
        .preferred_endpoint_id
        .and_then(|endpoint_id| candidate_target_by_endpoint(candidate, endpoint_id))
        .filter(|target| target.enabled)
    {
        let (key, invalid_override) = select_target_unit(
            target,
            request_prompt_log,
            stable_identity,
            quota_cache,
            model,
            estimated_tokens,
        );
        return Ok(NewSessionUnit {
            target,
            key,
            reason: db::RouteSelectionReason::SessionAffinity,
            invalid_override,
        });
    }

    let mut units = key_pool::candidate_units(candidate, Some(quota_cache), model, None);
    if units.is_empty() {
        units = key_pool::candidate_units_without_quota(candidate, None);
    }
    let unit = key_pool::draw(&units, stable_identity, Some(quota_cache), estimated_tokens)
        .ok_or_else(|| anyhow::Error::new(RouteAffinityError::target_unavailable(audit.clone())))?;
    let (key, invalid_override) = key_pool::apply_override(
        unit.target,
        unit.selection(),
        request_prompt_log.conversation_override_endpoint_key_id,
    );
    Ok(NewSessionUnit {
        target: unit.target,
        key,
        reason: db::RouteSelectionReason::SessionAffinity,
        invalid_override,
    })
}

fn select_target_unit(
    target: &db::ModelRouteCandidateTarget,
    request_prompt_log: &RequestPromptLog,
    stable_identity: &str,
    quota_cache: &TokenPlanQuotaCache,
    model: Option<&str>,
    estimated_tokens: u64,
) -> (db::EndpointApiKeySelection, bool) {
    let units = key_pool::target_units(target, Some(quota_cache), model);
    let drawn = key_pool::draw(&units, stable_identity, Some(quota_cache), estimated_tokens)
        .map(|unit| unit.selection())
        .unwrap_or_else(|| db::EndpointApiKeySelection {
            key_id: None,
            key_label: None,
            secret: target.api_key.clone(),
        });
    key_pool::apply_override(
        target,
        drawn,
        request_prompt_log.conversation_override_endpoint_key_id,
    )
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
