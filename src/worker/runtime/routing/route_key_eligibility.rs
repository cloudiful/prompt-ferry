use super::{
    key_pool::{self, RouteKeyUnit},
    quota_eligibility::{QuotaContext, QuotaPool, assess, reserve},
    quota_probe::{self, ProbeClaim, QuotaProbePermit},
    quota_selection::request_model,
};
use crate::{
    db,
    worker::runtime::{prompt_log::RequestPromptLog, request_assembly::BufferedBridgeRequest},
    worker_admin::token_plan_cache::{TokenPlanQuotaCache, estimate_input_tokens},
};

pub(super) fn eligible_route_keys(route: &db::RouteConfig) -> Vec<&db::EndpointApiKey> {
    let mut keys = route
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == route.route_id && key.enabled && !key.api_key.trim().is_empty()
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

fn route_units(
    route: &db::RouteConfig,
    cache: Option<&TokenPlanQuotaCache>,
    context: QuotaContext<'_>,
) -> QuotaPool<RouteKeyUnit> {
    let mut pool = QuotaPool::default();
    let keys = eligible_route_keys(route);
    if keys.is_empty() {
        if !route.api_key.trim().is_empty() {
            let key = db::EndpointApiKeySelection {
                key_id: route.endpoint_key_id,
                key_label: route.endpoint_key_label.clone(),
                secret: route.api_key.clone(),
            };
            pool.consider(
                assess(cache, route.route_id, key.key_id, context),
                |weight| RouteKeyUnit {
                    key,
                    unit_id: route.endpoint_key_id.unwrap_or(route.route_id),
                    weight,
                },
            );
        }
        return pool;
    }
    for key in keys
        .into_iter()
        .take(if route.key_lb_enabled { usize::MAX } else { 1 })
    {
        let selection = key_pool::selection_from_key(key);
        pool.consider(
            assess(cache, route.route_id, selection.key_id, context),
            |weight| RouteKeyUnit {
                key: selection,
                unit_id: key.key_id,
                weight,
            },
        );
    }
    pool
}

pub(in crate::worker::runtime) struct EndpointApiKeySelectionResult {
    pub selection: db::EndpointApiKeySelection,
    pub invalid_conversation_override: bool,
    pub quota_probe: Option<QuotaProbePermit>,
}

pub(super) fn materialize(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    log: &RequestPromptLog,
    cache: Option<&TokenPlanQuotaCache>,
) -> anyhow::Result<EndpointApiKeySelectionResult> {
    let model = request_model(request);
    let context = QuotaContext {
        model: model.as_deref(),
        path: &request.path,
    }
    .effective(route.upstream_model.as_deref());
    let invalid = log.conversation_override_endpoint_key_id.is_some_and(|id| {
        !eligible_route_keys(route)
            .iter()
            .any(|key| key.key_id == id)
    });
    let stable = super::selection::endpoint_key_stickiness_value(request, log);

    for _ in 0..quota_probe::route_claim_limit(route) {
        let override_selection = log
            .conversation_override_endpoint_key_id
            .and_then(|id| {
                eligible_route_keys(route)
                    .into_iter()
                    .find(|key| key.key_id == id)
            })
            .map(key_pool::selection_from_key)
            .filter(|selection| {
                assess(cache, route.route_id, selection.key_id, context)
                    .weight()
                    .is_some()
            });
        let selection = if let Some(selection) = override_selection {
            selection
        } else {
            let pool = route_units(route, cache, context);
            pool.checked()?;
            let unit = match stable.as_deref() {
                Some(stable) => key_pool::draw_route(&pool.units, stable),
                None => pool.units.first(),
            };
            unit.map(|unit| unit.key.clone())
                .ok_or_else(|| anyhow::anyhow!("route has no configured API key"))?
        };
        match quota_probe::claim(cache, route.route_id, selection.key_id) {
            ProbeClaim::NotRequired => {
                reserve(
                    cache,
                    route.route_id,
                    selection.key_id,
                    estimate_input_tokens(&request.body),
                );
                return Ok(EndpointApiKeySelectionResult {
                    selection,
                    invalid_conversation_override: invalid,
                    quota_probe: None,
                });
            }
            ProbeClaim::Reserved(permit) => {
                reserve(
                    cache,
                    route.route_id,
                    selection.key_id,
                    estimate_input_tokens(&request.body),
                );
                return Ok(EndpointApiKeySelectionResult {
                    selection,
                    invalid_conversation_override: invalid,
                    quota_probe: Some(permit),
                });
            }
            ProbeClaim::LostRace => continue,
        }
    }
    Err(crate::worker::runtime::routing::QuotaUnavailableError.into())
}
