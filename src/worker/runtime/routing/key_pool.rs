use super::quota_eligibility::{QuotaContext, QuotaPool, assess};
use crate::{db, worker_admin::token_plan_cache::TokenPlanQuotaCache};

pub(super) struct PoolUnit<'a> {
    pub target: &'a db::ModelRouteCandidateTarget,
    pub key: db::EndpointApiKeySelection,
    pub unit_id: uuid::Uuid,
    pub weight: f64,
}
impl PoolUnit<'_> {
    pub fn selection(&self) -> db::EndpointApiKeySelection {
        self.key.clone()
    }
}
pub(super) struct RouteKeyUnit {
    pub key: db::EndpointApiKeySelection,
    pub unit_id: uuid::Uuid,
    pub weight: f64,
}

pub(super) fn eligible_keys(target: &db::ModelRouteCandidateTarget) -> Vec<&db::EndpointApiKey> {
    let mut keys = target
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == target.endpoint_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    keys.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.key_label.cmp(&b.key_label))
            .then_with(|| a.key_id.cmp(&b.key_id))
    });
    keys
}
pub(super) fn selection_from_key(key: &db::EndpointApiKey) -> db::EndpointApiKeySelection {
    db::EndpointApiKeySelection {
        key_id: (!key.key_id.is_nil()).then_some(key.key_id),
        key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
        secret: key.api_key.clone(),
    }
}
pub(super) fn override_key(
    target: &db::ModelRouteCandidateTarget,
    id: uuid::Uuid,
) -> Option<db::EndpointApiKeySelection> {
    eligible_keys(target)
        .into_iter()
        .find(|key| key.key_id == id)
        .map(selection_from_key)
}

pub(super) fn target_units<'a>(
    target: &'a db::ModelRouteCandidateTarget,
    cache: Option<&TokenPlanQuotaCache>,
    context: QuotaContext<'_>,
    all_keys: bool,
) -> QuotaPool<PoolUnit<'a>> {
    let mut pool = QuotaPool::default();
    if !db::candidate_target_is_active(target, db::worker_local_minutes_now()) {
        return pool;
    }
    let context = context.effective(target.upstream_model.as_deref());
    let keys = eligible_keys(target);
    if keys.is_empty() {
        let secret = target.api_key.trim();
        if !secret.is_empty() {
            pool.consider(assess(cache, target.endpoint_id, None, context), |weight| {
                PoolUnit {
                    target,
                    key: db::EndpointApiKeySelection {
                        key_id: None,
                        key_label: None,
                        secret: secret.to_string(),
                    },
                    unit_id: target.target_id,
                    weight,
                }
            });
        }
        return pool;
    }
    for key in keys.into_iter().take(if all_keys || target.key_lb_enabled {
        usize::MAX
    } else {
        1
    }) {
        pool.consider(
            assess(
                cache,
                target.endpoint_id,
                (!key.key_id.is_nil()).then_some(key.key_id),
                context,
            ),
            |weight| PoolUnit {
                target,
                key: selection_from_key(key),
                unit_id: key.key_id,
                weight,
            },
        );
    }
    pool
}

pub(super) fn candidate_units<'a>(
    candidate: &'a db::ModelRouteCandidate,
    cache: Option<&TokenPlanQuotaCache>,
    context: QuotaContext<'_>,
    exclude: Option<uuid::Uuid>,
    all_keys: bool,
) -> QuotaPool<PoolUnit<'a>> {
    let mut pool = QuotaPool::default();
    for target in &candidate.targets {
        let mut target_pool = target_units(target, cache, context, all_keys);
        target_pool
            .units
            .retain(|unit| Some(unit.unit_id) != exclude && Some(unit.target.target_id) != exclude);
        pool.extend(target_pool);
    }
    pool
}

pub(super) fn draw<'a, 'b>(
    units: &'b [PoolUnit<'a>],
    stable_key: &str,
) -> Option<&'b PoolUnit<'a>> {
    let entries = units
        .iter()
        .map(|unit| (unit.unit_id, unit.weight))
        .collect::<Vec<_>>();
    crate::routing::unified_pool_draw(&entries, stable_key).map(|index| &units[index])
}

pub(super) fn draw_route<'a>(
    units: &'a [RouteKeyUnit],
    stable_key: &str,
) -> Option<&'a RouteKeyUnit> {
    let entries = units
        .iter()
        .map(|unit| (unit.unit_id, unit.weight))
        .collect::<Vec<_>>();
    crate::routing::unified_pool_draw(&entries, stable_key).map(|index| &units[index])
}
