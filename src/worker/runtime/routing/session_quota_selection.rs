use super::{
    candidate_quota_selection::{self, CandidateSelection, target_pool},
    key_pool,
    quota_eligibility::{QuotaContext, QuotaPool},
    session_affinity::RouteAffinityError,
};
use crate::{
    db,
    routing::candidate_target_by_endpoint,
    worker::runtime::{context::AffinityFailureAudit, prompt_log::RequestPromptLog},
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};

pub(super) fn select<'a>(
    candidate: &'a db::ModelRouteCandidate,
    log: &RequestPromptLog,
    stable: &str,
    cache: &TokenPlanQuotaCache,
    context: QuotaContext<'_>,
    audit: &AffinityFailureAudit,
) -> anyhow::Result<CandidateSelection<'a>> {
    if let Some(id) = log.conversation_override_endpoint_id {
        let target = candidate_target_by_endpoint(candidate, id)
            .filter(|target| db::candidate_target_is_active(target, db::worker_local_minutes_now()))
            .ok_or_else(|| RouteAffinityError::target_unavailable(audit.clone()))?;
        let pool = target_pool(
            target,
            Some(cache),
            context,
            log.conversation_override_endpoint_key_id,
        );
        if let Some(unit) = key_pool::draw(&pool.units, stable) {
            return Ok(candidate_quota_selection::selection(
                candidate,
                unit,
                log.conversation_override_endpoint_key_id,
                db::RouteSelectionReason::ConversationOverride,
            ));
        }
    }
    if let Some(target) = log
        .preferred_endpoint_id
        .and_then(|id| candidate_target_by_endpoint(candidate, id))
    {
        let pool = target_pool(
            target,
            Some(cache),
            context,
            log.conversation_override_endpoint_key_id,
        );
        if let Some(unit) = key_pool::draw(&pool.units, stable) {
            return Ok(candidate_quota_selection::selection(
                candidate,
                unit,
                log.conversation_override_endpoint_key_id,
                db::RouteSelectionReason::SessionAffinity,
            ));
        }
    }
    let mut pool = QuotaPool::default();
    for target in &candidate.targets {
        pool.extend(target_pool(
            target,
            Some(cache),
            context,
            log.conversation_override_endpoint_key_id,
        ));
    }
    pool.checked()?;
    let unit = key_pool::draw(&pool.units, stable)
        .ok_or_else(|| RouteAffinityError::target_unavailable(audit.clone()))?;
    Ok(candidate_quota_selection::selection(
        candidate,
        unit,
        log.conversation_override_endpoint_key_id,
        db::RouteSelectionReason::SessionAffinity,
    ))
}
