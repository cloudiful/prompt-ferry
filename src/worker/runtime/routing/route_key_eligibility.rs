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
    // Issue #1004 P1: a subscription route carries no platform key row. It
    // contributes exactly one key-id-less unit so the request can be routed,
    // and the unit bypasses the platform-key quota eligibility.
    if route.has_oauth_token {
        let key = db::EndpointApiKeySelection {
            key_id: None,
            key_label: None,
            secret: String::new(),
        };
        pool.consider(
            key_pool::subscription_eligibility(cache, route.route_id),
            |weight| RouteKeyUnit {
                key,
                unit_id: route.route_id,
                weight,
            },
        );
        return pool;
    }
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

/// Issue #1004 P1: actionable reason for a route without any credential. An
/// OpenAI endpoint without a stored key is either waiting for the ChatGPT
/// login or missing a Platform API key, so the message names both.
fn missing_credential_message(route: &db::RouteConfig) -> &'static str {
    if route.provider == db::EndpointProvider::OpenAi {
        "route has no configured API key; complete the ChatGPT login for this endpoint or store a Platform API key"
    } else {
        "route has no configured API key"
    }
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
                .ok_or_else(|| anyhow::anyhow!(missing_credential_message(route)))?
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::EndpointProvider;

    fn route(has_oauth_token: bool, api_key: &str) -> db::RouteConfig {
        db::RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://api.openai.com".to_string(),
            api_key: api_key.to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Responses,
            upstream_model: None,
            route_selection_reason: db::RouteSelectionReason::Default,
            provider: EndpointProvider::OpenAi,
            service_tier: None,
            proxy_url: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
            compact_mode: db::CompactMode::Passthrough,
            thinking_downgrade_enabled: false,
            has_oauth_token,
        }
    }

    fn request() -> BufferedBridgeRequest {
        BufferedBridgeRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            headers: Vec::new(),
            body: br#"{"model":"gpt-5","input":"ping"}"#.to_vec(),
            request_deadline_unix_ms: 0,
            user_id: Some(1),
            client_key_hash: None,
            request_user_agent: None,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
        }
    }

    fn context() -> QuotaContext<'static> {
        QuotaContext {
            model: None,
            path: "/v1/responses",
        }
    }

    #[test]
    fn a_subscription_route_contributes_exactly_one_keyless_unit() {
        let route = route(true, "");
        let pool = route_units(&route, None, context());
        assert_eq!(pool.units.len(), 1);
        let unit = &pool.units[0];
        assert_eq!(unit.key.key_id, None);
        assert!(
            unit.key.secret.is_empty(),
            "a subscription unit never carries a bearer secret"
        );
        assert_eq!(unit.unit_id, route.route_id);
    }

    #[test]
    fn a_keyless_route_without_a_login_contributes_no_unit() {
        let route = route(false, "");
        let pool = route_units(&route, None, context());
        assert!(
            pool.units.is_empty(),
            "an endpoint without any credential must not be routable"
        );
    }

    #[test]
    fn a_keyless_route_without_a_login_fails_with_an_actionable_prerequisite() {
        let route = route(false, "");
        let error = match materialize(&route, &request(), &RequestPromptLog::default(), None) {
            Ok(_) => panic!("a route without a credential cannot be materialized"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("ChatGPT login"),
            "the error must name the missing prerequisite: {error}"
        );
    }

    #[test]
    fn materializing_a_subscription_route_yields_a_keyless_selection() {
        let route = route(true, "");
        let selected = materialize(&route, &request(), &RequestPromptLog::default(), None)
            .expect("a subscription route is selectable");
        assert_eq!(selected.selection.key_id, None);
        assert!(selected.selection.secret.is_empty());
    }
}
