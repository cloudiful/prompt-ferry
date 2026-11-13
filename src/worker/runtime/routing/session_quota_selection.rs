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

/// Whether the route's new-session preference may steer a selection.
///
/// The preference only steers a session that has no binding at all. A
/// session whose binding was observed — including a stale binding that is
/// rebuilt or an existing binding that is rebound — follows the automatic
/// rules, so an established session is never repinned by the preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewSessionPreferencePermission {
    /// No binding was observed for this session.
    InitialBinding,
    /// A binding was observed for this session.
    ExistingBinding,
}

impl NewSessionPreferencePermission {
    pub(super) fn for_attempt(observed_existing_binding: bool) -> Self {
        if observed_existing_binding {
            Self::ExistingBinding
        } else {
            Self::InitialBinding
        }
    }

    pub(super) fn allows_preference(self) -> bool {
        matches!(self, Self::InitialBinding)
    }
}

pub(super) fn select<'a>(
    candidate: &'a db::ModelRouteCandidate,
    log: &RequestPromptLog,
    stable: &str,
    cache: &TokenPlanQuotaCache,
    context: QuotaContext<'_>,
    audit: &AffinityFailureAudit,
    permission: NewSessionPreferencePermission,
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
    // A route may steer future unbound sessions to one target.
    // The preference only participates in an initial binding; a session that
    // already has a binding follows the automatic rules, and a disabled,
    // out-of-window, cooling or exhausted target contributes no unit and the
    // automatic draw below binds normally without moving an existing session.
    if permission.allows_preference()
        && let Some(target) = candidate
            .targets
            .iter()
            .find(|target| target.prefer_new_sessions)
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
                db::RouteSelectionReason::NewSessionPreference,
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
