use super::quota_eligibility::{Eligibility, QuotaContext, QuotaPool, assess};
use crate::{
    db,
    worker_admin::{
        quota_feedback::{QuotaFeedbackEligibility, QuotaFeedbackScope},
        token_plan_cache::TokenPlanQuotaCache,
    },
};
use std::time::Instant;
use uuid::Uuid;

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
    if target.has_oauth_token {
        return None;
    }
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
    // Issue #1004 P1: a subscription target carries no platform key row. It
    // contributes exactly one key-id-less unit so the selector can draw it,
    // and the unit bypasses the platform-key quota eligibility (the
    // subscription quota is tracked per endpoint).
    if target.has_oauth_token {
        pool.consider(
            assess_target_credential(cache, target, None, context),
            |weight| PoolUnit {
                target,
                key: crate::routing::subscription_selection(),
                unit_id: target.target_id,
                weight,
            },
        );
        return pool;
    }
    let keys = eligible_keys(target);
    if keys.is_empty() {
        let secret = target.api_key.trim();
        if !secret.is_empty() {
            pool.consider(
                assess_target_credential(cache, target, None, context),
                |weight| PoolUnit {
                    target,
                    key: db::EndpointApiKeySelection {
                        key_id: None,
                        key_label: None,
                        secret: secret.to_string(),
                    },
                    unit_id: target.target_id,
                    weight,
                },
            );
        }
        return pool;
    }
    for key in keys.into_iter().take(if all_keys || target.key_lb_enabled {
        usize::MAX
    } else {
        1
    }) {
        pool.consider(
            assess_target_credential(
                cache,
                target,
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

/// Issue #1004 P1: eligibility of a ChatGPT subscription unit.
///
/// A subscription endpoint has no platform API key, so the platform-key
/// quota scopes (the per-key remaining and weight budgets) do not apply to
/// it. The endpoint-level feedback backoff and the subscription quota still
/// do, so an exhausted subscription keeps its target out of the pool.
pub(super) fn assess_target_credential(
    cache: Option<&TokenPlanQuotaCache>,
    target: &db::ModelRouteCandidateTarget,
    key_id: Option<Uuid>,
    context: QuotaContext<'_>,
) -> Eligibility {
    if target.has_oauth_token {
        subscription_eligibility(cache, target.endpoint_id)
    } else {
        assess(cache, target.endpoint_id, key_id, context)
    }
}

pub(super) fn subscription_eligibility(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint: Uuid,
) -> Eligibility {
    let Some(cache) = cache else {
        return Eligibility::Eligible(1.0);
    };
    match cache
        .feedback
        .eligibility(QuotaFeedbackScope::new(endpoint, None), Instant::now())
    {
        QuotaFeedbackEligibility::Cooling => return Eligibility::Cooling,
        QuotaFeedbackEligibility::ProbeBusy => return Eligibility::ProbeBusy,
        QuotaFeedbackEligibility::Eligible | QuotaFeedbackEligibility::ProbeRequired => {}
    }
    if cache.chatgpt_subscription_exhausted_now(endpoint) {
        return Eligibility::Exhausted;
    }
    Eligibility::Eligible(1.0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::EndpointProvider;

    fn endpoint_key(endpoint_id: uuid::Uuid, key_id: uuid::Uuid) -> db::EndpointApiKey {
        db::EndpointApiKey {
            key_id,
            endpoint_id,
            key_label: "primary".to_string(),
            api_key: "sk-platform".to_string(),
            position: 0,
            enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn target(
        has_oauth_token: bool,
        keys: Vec<db::EndpointApiKey>,
    ) -> db::ModelRouteCandidateTarget {
        let endpoint_id = keys
            .first()
            .map(|key| key.endpoint_id)
            .unwrap_or_else(uuid::Uuid::new_v4);
        db::ModelRouteCandidateTarget {
            embedding_dimensions_override: None,
            target_id: uuid::Uuid::new_v4(),
            endpoint_id,
            endpoint_name: "endpoint".to_string(),
            base_url: "https://api.openai.com".to_string(),
            api_key: keys
                .first()
                .map(|key| key.api_key.clone())
                .unwrap_or_default(),
            api_keys: keys,
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Responses,
            target_native_api: crate::config::NativeApi::Responses,
            position: 0,
            enabled: true,
            upstream_model: None,
            provider: EndpointProvider::OpenAi,
            service_tier: None,
            proxy_url: None,
            proxy_url_override: None,
            active_windows: None,
            endpoint_active_windows: None,
            dev_system_normalize: false,
            thinking_downgrade_enabled: false,
            thinking_effort_override: None,
            compact_mode: crate::db::CompactMode::Passthrough,
            has_oauth_token,
        }
    }

    fn context() -> QuotaContext<'static> {
        QuotaContext {
            model: None,
            path: "/v1/responses",
        }
    }

    #[test]
    fn a_subscription_target_contributes_exactly_one_keyless_unit() {
        let target = target(true, Vec::new());
        let pool = target_units(&target, None, context(), false);
        assert_eq!(pool.units.len(), 1);
        let unit = &pool.units[0];
        assert_eq!(unit.key.key_id, None);
        assert_eq!(unit.key.key_label, None);
        assert!(
            unit.key.secret.is_empty(),
            "a subscription unit never carries a bearer secret"
        );
        assert_eq!(unit.unit_id, target.target_id);
    }

    #[test]
    fn subscription_binding_ignores_unused_platform_material_and_rejects_invalid_shapes() {
        let key = endpoint_key(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        let target = target(true, vec![key.clone()]);
        let unit = target_units(&target, None, context(), false)
            .units
            .pop()
            .unwrap();
        let binding = crate::routing::credential_binding(&target, &unit.key);
        let resolved = crate::routing::select_bound_credential(&target, &binding).unwrap();
        assert_eq!(resolved.key_id, None);
        assert!(resolved.secret.is_empty());
        assert!(override_key(&target, key.key_id).is_none());
        let mut invalid = binding.clone();
        invalid.endpoint_key_id = Some(key.key_id);
        assert!(crate::routing::select_bound_credential(&target, &invalid).is_none());
        invalid.endpoint_key_id = None;
        invalid.endpoint_key_fingerprint = crate::response_affinity::api_key_fingerprint("foreign");
        assert!(crate::routing::select_bound_credential(&target, &invalid).is_none());
    }

    #[test]
    fn a_keyless_target_without_a_login_contributes_no_unit() {
        let target = target(false, Vec::new());
        let pool = target_units(&target, None, context(), false);
        assert!(
            pool.units.is_empty(),
            "an endpoint without any credential must not be routable"
        );
    }

    #[test]
    fn a_platform_target_keeps_its_key_unit() {
        let endpoint_id = uuid::Uuid::new_v4();
        let key = endpoint_key(endpoint_id, uuid::Uuid::new_v4());
        let target = target(false, vec![key.clone()]);
        let pool = target_units(&target, None, context(), false);
        assert_eq!(pool.units.len(), 1);
        assert_eq!(pool.units[0].key.key_id, Some(key.key_id));
        assert_eq!(pool.units[0].key.secret, "sk-platform");
    }
}
