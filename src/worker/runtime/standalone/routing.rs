use chrono::Utc;

use crate::{
    db::{self, ModelRouteCandidate, ModelRouteCandidateTarget},
    standalone_config::{
        ContinuationPolicy, EndpointApiKeyConfig, ModelRouteConfig, ProviderEndpointConfig,
        RouteScope, RoutingStrategy, StandaloneConfig,
    },
};

pub(crate) fn standalone_model_route_candidate(
    snapshot: &StandaloneConfig,
    user_id: i64,
    model: Option<&str>,
) -> Option<ModelRouteCandidate> {
    let model = model?;
    snapshot
        .routes
        .iter()
        .enumerate()
        .filter_map(|(index, route)| {
            if !route.enabled || !route_scope_applies(route, user_id) {
                return None;
            }
            let specificity = pattern_specificity(&route.model_pattern, model)?;
            let candidate = candidate_from_route(snapshot, route)?;
            Some((index, candidate, specificity))
        })
        .max_by(
            |(left_index, left, left_specificity), (right_index, right, right_specificity)| {
                route_precedence(left, *left_specificity, *left_index).cmp(&route_precedence(
                    right,
                    *right_specificity,
                    *right_index,
                ))
            },
        )
        .map(|(_, candidate, _)| candidate)
}

fn candidate_from_route(
    snapshot: &StandaloneConfig,
    route: &ModelRouteConfig,
) -> Option<ModelRouteCandidate> {
    let targets = route
        .targets
        .iter()
        .filter(|target| target.enabled)
        .filter_map(|target| {
            let endpoint = snapshot
                .endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == target.endpoint_id && endpoint.enabled)?;
            Some(target_from_endpoint(endpoint, target))
        })
        .collect::<Vec<_>>();
    (!targets.is_empty()).then_some(ModelRouteCandidate {
        rule_id: route.rule_id,
        scope: match route.scope {
            RouteScope::Admin => "admin".to_string(),
            RouteScope::User => "user".to_string(),
        },
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern.clone(),
        routing_strategy: match route.routing_strategy {
            RoutingStrategy::ClientKeyRendezvous => {
                db::ModelRouteRoutingStrategy::ClientKeyRendezvous
            }
            RoutingStrategy::ResponsesSessionAffinity => {
                db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
            }
        },
        daily_max_requests: route.daily_max_requests,
        monthly_max_requests: route.monthly_max_requests,
        updated_at: Utc::now(),
        targets,
    })
}

fn target_from_endpoint(
    endpoint: &ProviderEndpointConfig,
    target: &crate::standalone_config::ModelRouteTargetConfig,
) -> ModelRouteCandidateTarget {
    ModelRouteCandidateTarget {
        target_id: target.target_id,
        endpoint_id: endpoint.endpoint_id,
        endpoint_name: endpoint.name.clone(),
        base_url: endpoint.base_url.clone(),
        api_key: endpoint.api_key.clone(),
        api_keys: endpoint
            .api_keys
            .iter()
            .map(|key| endpoint_key(endpoint.endpoint_id, key))
            .collect(),
        key_lb_enabled: endpoint.key_lb_enabled,
        native_api: endpoint.native_api,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model.clone(),
        responses_continuation_policy: match target.responses_continuation_policy {
            ContinuationPolicy::ForcePassthrough => {
                db::ResponsesContinuationPolicy::ForcePassthrough
            }
            ContinuationPolicy::ForceReplay => db::ResponsesContinuationPolicy::ForceReplay,
        },
        provider: match endpoint.provider {
            crate::standalone_config::EndpointProvider::Minimax => db::EndpointProvider::Minimax,
            crate::standalone_config::EndpointProvider::Generic => db::EndpointProvider::Generic,
        },
        service_tier: match endpoint.service_tier {
            crate::standalone_config::MinimaxServiceTier::Priority => {
                db::MinimaxServiceTier::Priority
            }
            crate::standalone_config::MinimaxServiceTier::Standard => {
                db::MinimaxServiceTier::Standard
            }
        },
    }
}

fn endpoint_key(endpoint_id: uuid::Uuid, key: &EndpointApiKeyConfig) -> db::EndpointApiKey {
    db::EndpointApiKey {
        key_id: key.key_id,
        endpoint_id,
        key_label: key.key_label.clone(),
        api_key: key.api_key.clone(),
        position: key.position,
        enabled: key.enabled,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn route_scope_applies(route: &ModelRouteConfig, user_id: i64) -> bool {
    match route.scope {
        RouteScope::Admin => true,
        RouteScope::User => route.owner_user_id == Some(user_id),
    }
}

fn pattern_specificity(pattern: &str, model: &str) -> Option<(u8, usize)> {
    let pattern = pattern.trim();
    if pattern == "*" {
        return Some((0, 0));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix).then_some((1, prefix.len()));
    }
    (pattern == model).then_some((2, pattern.len()))
}

fn route_precedence(
    route: &ModelRouteCandidate,
    specificity: (u8, usize),
    index: usize,
) -> (u8, u8, usize, usize) {
    (
        u8::from(route.scope == "user"),
        specificity.0,
        specificity.1,
        usize::MAX - index,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{NativeApi, NativeApiSource};
    use crate::standalone_config::{
        EndpointProvider, EndpointRegion, MinimaxServiceTier, ModelRouteTargetConfig,
        ProviderEndpointConfig,
    };
    use uuid::Uuid;

    #[test]
    fn local_model_pattern_precedence_matches_managed_routing() {
        let endpoint_id = Uuid::new_v4();
        let snapshot = StandaloneConfig {
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "endpoint".to_string(),
                provider: EndpointProvider::Generic,
                provider_region: Some(EndpointRegion::Global),
                service_tier: MinimaxServiceTier::Standard,
                base_url: "https://example.test".to_string(),
                native_api: NativeApi::Responses,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: "key".to_string(),
                api_keys: Vec::new(),
            }],
            routes: vec![
                ModelRouteConfig {
                    rule_id: Uuid::new_v4(),
                    scope: RouteScope::Admin,
                    owner_user_id: None,
                    model_pattern: "gpt-*".to_string(),
                    routing_strategy: RoutingStrategy::ClientKeyRendezvous,
                    daily_max_requests: None,
                    monthly_max_requests: None,
                    enabled: true,
                    targets: vec![ModelRouteTargetConfig {
                        target_id: Uuid::new_v4(),
                        endpoint_id,
                        position: 0,
                        enabled: true,
                        upstream_model: None,
                        responses_continuation_policy: ContinuationPolicy::ForceReplay,
                    }],
                },
                ModelRouteConfig {
                    rule_id: Uuid::new_v4(),
                    scope: RouteScope::Admin,
                    owner_user_id: None,
                    model_pattern: "gpt-5".to_string(),
                    routing_strategy: RoutingStrategy::ClientKeyRendezvous,
                    daily_max_requests: None,
                    monthly_max_requests: None,
                    enabled: true,
                    targets: vec![ModelRouteTargetConfig {
                        target_id: Uuid::new_v4(),
                        endpoint_id,
                        position: 0,
                        enabled: true,
                        upstream_model: None,
                        responses_continuation_policy: ContinuationPolicy::ForceReplay,
                    }],
                },
            ],
            ..StandaloneConfig::default()
        };

        let candidate =
            standalone_model_route_candidate(&snapshot, 1, Some("gpt-5")).expect("matching route");
        assert_eq!(candidate.model_pattern, "gpt-5");
    }

    #[test]
    fn standalone_candidate_propagates_minimax_service_tier() {
        use crate::standalone_config::EndpointProvider as ScProvider;
        let endpoint_id = Uuid::new_v4();
        let snapshot = StandaloneConfig {
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "minimax".to_string(),
                provider: ScProvider::Minimax,
                provider_region: Some(EndpointRegion::Global),
                service_tier: MinimaxServiceTier::Priority,
                base_url: "https://api.minimaxi.com".to_string(),
                native_api: NativeApi::Chat,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: "key".to_string(),
                api_keys: Vec::new(),
            }],
            routes: vec![ModelRouteConfig {
                rule_id: Uuid::new_v4(),
                scope: RouteScope::Admin,
                owner_user_id: None,
                model_pattern: "*".to_string(),
                routing_strategy: RoutingStrategy::ClientKeyRendezvous,
                daily_max_requests: None,
                monthly_max_requests: None,
                enabled: true,
                targets: vec![ModelRouteTargetConfig {
                    target_id: Uuid::new_v4(),
                    endpoint_id,
                    position: 0,
                    enabled: true,
                    upstream_model: None,
                    responses_continuation_policy: ContinuationPolicy::ForceReplay,
                }],
            }],
            ..StandaloneConfig::default()
        };
        let candidate =
            standalone_model_route_candidate(&snapshot, 1, Some("anything")).expect("candidate");
        let target = &candidate.targets[0];
        assert_eq!(target.provider, db::EndpointProvider::Minimax);
        assert_eq!(target.service_tier, db::MinimaxServiceTier::Priority);
    }
}
