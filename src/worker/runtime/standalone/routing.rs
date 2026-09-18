use chrono::Utc;

use crate::{
    db::{self, ModelRouteCandidate, ModelRouteCandidateTarget},
    standalone_config::{
        EndpointApiKeyConfig, ModelRouteConfig, ProviderEndpointConfig, RouteScope,
        RoutingStrategy, StandaloneConfig,
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
        // Issue #466: only `responses_session_affinity` remains.
        routing_strategy: match route.routing_strategy {
            RoutingStrategy::ResponsesSessionAffinity => {
                db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
            }
        },
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
        // Issue #409 Phase 1: target explicit wins, else endpoint fallback.
        // Both `Auto` stays `Auto` for per-caller `resolve_auto_protocol`.
        native_api: db::resolve_target_native_api(target.native_api, endpoint.native_api),
        target_native_api: target.native_api,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model.clone(),
        provider: match endpoint.provider {
            crate::standalone_config::EndpointProvider::Minimax => db::EndpointProvider::Minimax,
            crate::standalone_config::EndpointProvider::CommandCode => {
                db::EndpointProvider::CommandCode
            }
            crate::standalone_config::EndpointProvider::OpencodeGo => {
                db::EndpointProvider::OpencodeGo
            }
            crate::standalone_config::EndpointProvider::OpenRouter => {
                db::EndpointProvider::OpenRouter
            }
            crate::standalone_config::EndpointProvider::Glm => db::EndpointProvider::Glm,
            crate::standalone_config::EndpointProvider::DeepSeek => db::EndpointProvider::DeepSeek,
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
        proxy_url: endpoint.proxy_url.clone(),
        proxy_url_override: target.proxy_url_override.clone(),
        active_windows: target.active_windows.clone(),
        // Issue #392 Phase K: endpoint default for inheritance + normalize.
        endpoint_active_windows: endpoint.active_windows.clone(),
        dev_system_normalize: target.dev_system_normalize,
        // Issue #464: per-target thinking effort override; None = inherit.
        thinking_effort_override: target.thinking_effort_override.clone(),
        // Issue #502 Task 5: per-target compact mode; missing = passthrough.
        compact_mode: db::resolve_target_compact_mode(Some(target.compact_mode.as_str())),
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
                proxy_url: None,
                active_windows: None,
            }],
            routes: vec![
                ModelRouteConfig {
                    rule_id: Uuid::new_v4(),
                    scope: RouteScope::Admin,
                    owner_user_id: None,
                    model_pattern: "gpt-*".to_string(),
                    routing_strategy: RoutingStrategy::ResponsesSessionAffinity,
                    enabled: true,
                    targets: vec![ModelRouteTargetConfig {
                        target_id: Uuid::new_v4(),
                        endpoint_id,
                        position: 0,
                        enabled: true,
                        upstream_model: None,
                        native_api: NativeApi::Auto,
                        proxy_url_override: None,
                        active_windows: None,
                        dev_system_normalize: false,
                        thinking_effort_override: None,
                        compact_mode: "passthrough".to_string(),
                    }],
                },
                ModelRouteConfig {
                    rule_id: Uuid::new_v4(),
                    scope: RouteScope::Admin,
                    owner_user_id: None,
                    model_pattern: "gpt-5".to_string(),
                    routing_strategy: RoutingStrategy::ResponsesSessionAffinity,
                    enabled: true,
                    targets: vec![ModelRouteTargetConfig {
                        target_id: Uuid::new_v4(),
                        endpoint_id,
                        position: 0,
                        enabled: true,
                        upstream_model: None,
                        native_api: NativeApi::Auto,
                        proxy_url_override: None,
                        active_windows: None,
                        dev_system_normalize: false,
                        thinking_effort_override: None,
                        compact_mode: "passthrough".to_string(),
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
                proxy_url: None,
                active_windows: None,
            }],
            routes: vec![ModelRouteConfig {
                rule_id: Uuid::new_v4(),
                scope: RouteScope::Admin,
                owner_user_id: None,
                model_pattern: "*".to_string(),
                routing_strategy: RoutingStrategy::ResponsesSessionAffinity,
                enabled: true,
                targets: vec![ModelRouteTargetConfig {
                    target_id: Uuid::new_v4(),
                    endpoint_id,
                    position: 0,
                    enabled: true,
                    upstream_model: None,
                    native_api: NativeApi::Auto,
                    proxy_url_override: None,
                    active_windows: None,
                    dev_system_normalize: false,
                    thinking_effort_override: None,
                    compact_mode: "passthrough".to_string(),
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
