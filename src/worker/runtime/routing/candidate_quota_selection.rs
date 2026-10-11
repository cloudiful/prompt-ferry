use super::{
    key_pool::{self, PoolUnit},
    quota_eligibility::{QuotaContext, QuotaPool, assess},
    quota_unavailable::QuotaUnavailableError,
};
use crate::{db, worker_admin::token_plan_cache::TokenPlanQuotaCache};

pub(super) struct CandidateSelection<'a> {
    pub target: &'a db::ModelRouteCandidateTarget,
    pub key: db::EndpointApiKeySelection,
    pub reason: db::RouteSelectionReason,
    pub invalid_override: bool,
}

pub(super) fn target_pool<'a>(
    target: &'a db::ModelRouteCandidateTarget,
    cache: Option<&TokenPlanQuotaCache>,
    context: QuotaContext<'_>,
    override_id: Option<uuid::Uuid>,
) -> QuotaPool<PoolUnit<'a>> {
    let mut pool = key_pool::target_units(target, cache, context, false);
    if !db::candidate_target_is_active(target, db::worker_local_minutes_now()) {
        return pool;
    }
    if let Some(id) = override_id
        && let Some(key) = key_pool::override_key(target, id)
        && let Some(weight) = assess(
            cache,
            target.endpoint_id,
            key.key_id,
            context.effective(target.upstream_model.as_deref()),
        )
        .weight()
    {
        pool.units.clear();
        pool.units.push(PoolUnit {
            target,
            key,
            unit_id: id,
            weight,
        });
    }
    pool
}

pub(super) fn select<'a>(
    candidate: &'a db::ModelRouteCandidate,
    cache: Option<&TokenPlanQuotaCache>,
    context: QuotaContext<'_>,
    stable: &str,
    endpoint_override: Option<uuid::Uuid>,
    key_override: Option<uuid::Uuid>,
) -> Result<Option<CandidateSelection<'a>>, QuotaUnavailableError> {
    if let Some(target) = endpoint_override.and_then(|id| {
        candidate
            .targets
            .iter()
            .find(|target| target.endpoint_id == id)
    }) {
        let pool = target_pool(target, cache, context, key_override);
        if let Some(unit) = key_pool::draw(&pool.units, stable) {
            return Ok(Some(selection(
                candidate,
                unit,
                key_override,
                db::RouteSelectionReason::ConversationOverride,
            )));
        }
    }
    let mut pool = QuotaPool::default();
    for target in &candidate.targets {
        pool.extend(target_pool(target, cache, context, key_override));
    }
    pool.checked()?;
    Ok(key_pool::draw(&pool.units, stable).map(|unit| {
        selection(
            candidate,
            unit,
            key_override,
            db::RouteSelectionReason::Default,
        )
    }))
}

pub(super) fn selection<'a>(
    candidate: &db::ModelRouteCandidate,
    unit: &PoolUnit<'a>,
    key_override: Option<uuid::Uuid>,
    reason: db::RouteSelectionReason,
) -> CandidateSelection<'a> {
    CandidateSelection {
        target: unit.target,
        key: unit.selection(),
        reason,
        invalid_override: key_override.is_some_and(|id| {
            !candidate.targets.iter().any(|target| {
                db::candidate_target_is_active(target, db::worker_local_minutes_now())
                    && key_pool::override_key(target, id).is_some()
            })
        }),
    }
}
