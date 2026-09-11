use crate::{db, worker_admin::token_plan_cache::TokenPlanQuotaCache};

/// One eligible routing unit in the unified key pool: a candidate target
/// paired with the API key that will be used on it, plus the weight the
/// deterministic draw assigns to the pair.
pub(super) struct PoolUnit<'a> {
    pub(super) target: &'a db::ModelRouteCandidateTarget,
    pub(super) key: db::EndpointApiKeySelection,
    pub(super) unit_id: uuid::Uuid,
    pub(super) weight: f64,
}

impl PoolUnit<'_> {
    pub(super) fn selection(&self) -> db::EndpointApiKeySelection {
        self.key.clone()
    }
}

/// A single-endpoint key unit used when a route has already been resolved
/// (fallback route, live quota failover). Mirrors [`PoolUnit`] without a
/// candidate target reference.
pub(super) struct RouteKeyUnit {
    pub(super) key: db::EndpointApiKeySelection,
    pub(super) unit_id: uuid::Uuid,
    pub(super) weight: f64,
}

enum KeyWeight {
    Exhausted,
    Known(f64),
    Unknown,
}

fn key_weight(
    quota_cache: Option<&TokenPlanQuotaCache>,
    endpoint_id: uuid::Uuid,
    key_id: uuid::Uuid,
    model: Option<&str>,
) -> KeyWeight {
    let Some(remaining) =
        quota_cache.and_then(|cache| cache.key_weight_percent_now(endpoint_id, key_id, model))
    else {
        return KeyWeight::Unknown;
    };
    if !remaining.is_finite() {
        return KeyWeight::Unknown;
    }
    let remaining = remaining.clamp(0.0, 100.0);
    if remaining > 0.0 {
        KeyWeight::Known(remaining)
    } else {
        KeyWeight::Exhausted
    }
}

fn eligible_keys(target: &db::ModelRouteCandidateTarget) -> Vec<&db::EndpointApiKey> {
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
    keys
}

fn selection_from_key(key: &db::EndpointApiKey) -> db::EndpointApiKeySelection {
    db::EndpointApiKeySelection {
        key_id: (!key.key_id.is_nil()).then_some(key.key_id),
        key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
        secret: key.api_key.clone(),
    }
}

/// Resolve an explicit conversation key override against every eligible
/// key of the target, independent of the load-balancing setting.
pub(super) fn override_key(
    target: &db::ModelRouteCandidateTarget,
    key_id: uuid::Uuid,
) -> Option<db::EndpointApiKeySelection> {
    eligible_keys(target)
        .into_iter()
        .find(|key| key.key_id == key_id)
        .map(selection_from_key)
}

pub(super) fn override_route_key(
    route: &db::RouteConfig,
    key_id: uuid::Uuid,
) -> Option<db::EndpointApiKeySelection> {
    eligible_route_keys(route)
        .into_iter()
        .find(|key| key.key_id == key_id)
        .map(selection_from_key)
}

/// Apply an explicit conversation key override to a target. Returns the
/// override selection when it resolves to an eligible key, otherwise the
/// drawn selection with `invalid=true` so the caller can clear the stale
/// override.
pub(super) fn apply_override(
    target: &db::ModelRouteCandidateTarget,
    drawn: db::EndpointApiKeySelection,
    override_key_id: Option<uuid::Uuid>,
) -> (db::EndpointApiKeySelection, bool) {
    let Some(key_id) = override_key_id else {
        return (drawn, false);
    };
    match override_key(target, key_id) {
        Some(selection) => (selection, false),
        None => (drawn, true),
    }
}

fn secret_unit<'a>(target: &'a db::ModelRouteCandidateTarget) -> Option<PoolUnit<'a>> {
    let secret = target.api_key.trim();
    if secret.is_empty() {
        return None;
    }
    Some(PoolUnit {
        target,
        key: db::EndpointApiKeySelection {
            key_id: None,
            key_label: None,
            secret: secret.to_string(),
        },
        unit_id: target.target_id,
        weight: 1.0,
    })
}

/// One eligibility filter for a whole target: disabled targets and empty
/// secrets drop out, exhausted keys drop out, and a target without eligible
/// per-key rows falls back to its single secret with weight 1.0. Key
/// load-balancing disabled keeps only the primary key so the target stays
/// deterministic; failover passes `include_all_keys` to rotate past an
/// exhausted primary.
pub(super) fn target_units<'a>(
    target: &'a db::ModelRouteCandidateTarget,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
) -> Vec<PoolUnit<'a>> {
    target_units_mode(target, quota_cache, model, false)
}

fn target_units_mode<'a>(
    target: &'a db::ModelRouteCandidateTarget,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    include_all_keys: bool,
) -> Vec<PoolUnit<'a>> {
    if !target.enabled {
        return Vec::new();
    }
    let keys = eligible_keys(target);
    if keys.is_empty() {
        return secret_unit(target).into_iter().collect();
    }
    let chosen = if include_all_keys || target.key_lb_enabled {
        keys
    } else {
        vec![keys[0]]
    };
    chosen
        .into_iter()
        .filter_map(|key| {
            let weight = match key_weight(quota_cache, target.endpoint_id, key.key_id, model) {
                KeyWeight::Exhausted => return None,
                KeyWeight::Known(weight) => weight,
                KeyWeight::Unknown => 1.0,
            };
            Some(PoolUnit {
                target,
                key: selection_from_key(key),
                unit_id: key.key_id,
                weight,
            })
        })
        .collect()
}

/// Flatten every enabled target of the candidate into one pool. When
/// `exclude_unit` is set the matching unit is removed before the redraw
/// (used when the bound unit's quota is exhausted).
pub(super) fn candidate_units<'a>(
    candidate: &'a db::ModelRouteCandidate,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    exclude_unit: Option<uuid::Uuid>,
) -> Vec<PoolUnit<'a>> {
    candidate_units_mode(candidate, quota_cache, model, exclude_unit, false)
}

/// Failover pool: every eligible key is included regardless of the per
/// endpoint load-balancing setting so an exhausted primary can be rotated.
pub(super) fn candidate_units_all_keys<'a>(
    candidate: &'a db::ModelRouteCandidate,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    exclude_unit: Option<uuid::Uuid>,
) -> Vec<PoolUnit<'a>> {
    candidate_units_mode(candidate, quota_cache, model, exclude_unit, true)
}

fn candidate_units_mode<'a>(
    candidate: &'a db::ModelRouteCandidate,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    exclude_unit: Option<uuid::Uuid>,
    include_all_keys: bool,
) -> Vec<PoolUnit<'a>> {
    candidate
        .targets
        .iter()
        .flat_map(|target| target_units_mode(target, quota_cache, model, include_all_keys))
        .filter(|unit| Some(unit.unit_id) != exclude_unit)
        .collect()
}

/// Same as [`candidate_units`] but ignores quota exhaustion so a candidate
/// whose keys are all exhausted still resolves a route instead of failing.
pub(super) fn candidate_units_without_quota<'a>(
    candidate: &'a db::ModelRouteCandidate,
    exclude_unit: Option<uuid::Uuid>,
) -> Vec<PoolUnit<'a>> {
    candidate_units_mode(candidate, None, None, exclude_unit, false)
}

pub(super) fn candidate_units_without_quota_all_keys<'a>(
    candidate: &'a db::ModelRouteCandidate,
    exclude_unit: Option<uuid::Uuid>,
) -> Vec<PoolUnit<'a>> {
    candidate_units_mode(candidate, None, None, exclude_unit, true)
}

/// Deterministic weighted draw over candidate pool units. The chosen
/// MiniMax-style unit reserves its estimated tokens so concurrent requests
/// do not overshoot the same key.
pub(super) fn draw<'a, 'b>(
    units: &'b [PoolUnit<'a>],
    stable_key: &str,
    quota_cache: Option<&TokenPlanQuotaCache>,
    estimated_tokens: u64,
) -> Option<&'b PoolUnit<'a>> {
    let entries = units
        .iter()
        .map(|unit| (unit.unit_id, unit.weight))
        .collect::<Vec<_>>();
    let index = crate::routing::unified_pool_draw(&entries, stable_key)?;
    let unit = &units[index];
    if let (Some(cache), Some(key_id)) = (quota_cache, unit.key.key_id) {
        cache.reserve_estimated_tokens(unit.target.endpoint_id, key_id, estimated_tokens);
    }
    Some(unit)
}

fn eligible_route_keys(route: &db::RouteConfig) -> Vec<&db::EndpointApiKey> {
    let mut keys = route
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == route.route_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    keys.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.key_label.cmp(&right.key_label))
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    keys
}

/// Build the unified pool for an already-resolved single-endpoint route.
pub(super) fn route_units(
    route: &db::RouteConfig,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
) -> Vec<RouteKeyUnit> {
    let keys = eligible_route_keys(route);
    if keys.is_empty() {
        let secret = route.api_key.trim();
        if secret.is_empty() {
            return Vec::new();
        }
        return vec![RouteKeyUnit {
            key: db::EndpointApiKeySelection {
                key_id: None,
                key_label: None,
                secret: secret.to_string(),
            },
            unit_id: route.route_id,
            weight: 1.0,
        }];
    }
    let chosen = if route.key_lb_enabled {
        keys
    } else {
        vec![keys[0]]
    };
    chosen
        .into_iter()
        .filter_map(|key| {
            let weight = match key_weight(quota_cache, route.route_id, key.key_id, model) {
                KeyWeight::Exhausted => return None,
                KeyWeight::Known(weight) => weight,
                KeyWeight::Unknown => 1.0,
            };
            Some(RouteKeyUnit {
                key: selection_from_key(key),
                unit_id: key.key_id,
                weight,
            })
        })
        .collect()
}

pub(super) fn draw_route<'a>(
    units: &'a [RouteKeyUnit],
    stable_key: &str,
    route_id: uuid::Uuid,
    quota_cache: Option<&TokenPlanQuotaCache>,
    estimated_tokens: u64,
) -> Option<&'a RouteKeyUnit> {
    let entries = units
        .iter()
        .map(|unit| (unit.unit_id, unit.weight))
        .collect::<Vec<_>>();
    let index = crate::routing::unified_pool_draw(&entries, stable_key)?;
    let unit = &units[index];
    if let (Some(cache), Some(key_id)) = (quota_cache, unit.key.key_id) {
        cache.reserve_estimated_tokens(route_id, key_id, estimated_tokens);
    }
    Some(unit)
}
