//! Mapping helpers between the unified model route DTOs and the
//! PostgreSQL/SQLite row models.

use anyhow::Result;
use uuid::Uuid;

use crate::db::{
    ModelEndpointRule as PgModelEndpointRule, ModelEndpointRuleCreate, ModelRouteRoutingStrategy,
    ResponsesContinuationPolicy,
};
use crate::standalone_config::{
    ContinuationPolicy as ScContinuationPolicy, ModelRouteConfig as ScModelRoute,
    RouteScope as ScRouteScope, RoutingStrategy as ScRoutingStrategy,
};

use super::{UnifiedModelRoute, UnifiedModelRouteTarget};

pub(super) fn from_postgres(rule: PgModelEndpointRule) -> UnifiedModelRoute {
    UnifiedModelRoute {
        rule_id: rule.rule_id,
        scope: rule.scope,
        owner_user_id: rule.owner_user_id,
        model_pattern: rule.model_pattern,
        routing_strategy: rule.routing_strategy,
        daily_max_requests: rule.daily_max_requests,
        monthly_max_requests: rule.monthly_max_requests,
        enabled: rule.enabled,
        targets: rule.targets.into_iter().map(from_postgres_target).collect(),
    }
}

pub(super) fn from_postgres_target(target: crate::db::ModelRouteTarget) -> UnifiedModelRouteTarget {
    UnifiedModelRouteTarget {
        target_id: target.target_id,
        rule_id: target.rule_id,
        endpoint_id: target.endpoint_id,
        endpoint_name: target.endpoint_name,
        endpoint_enabled: target.endpoint_enabled,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model,
        responses_continuation_policy: target.responses_continuation_policy,
    }
}

pub(super) fn scope_to_string(scope: ScRouteScope) -> String {
    match scope {
        ScRouteScope::Admin => "admin".to_string(),
        ScRouteScope::User => "user".to_string(),
    }
}

pub(super) fn routing_strategy_to_pg(strategy: ScRoutingStrategy) -> ModelRouteRoutingStrategy {
    match strategy {
        ScRoutingStrategy::ClientKeyRendezvous => ModelRouteRoutingStrategy::ClientKeyRendezvous,
        ScRoutingStrategy::ResponsesSessionAffinity => {
            ModelRouteRoutingStrategy::ResponsesSessionAffinity
        }
    }
}

pub(super) fn continuation_policy_to_pg(
    policy: ScContinuationPolicy,
) -> ResponsesContinuationPolicy {
    match policy {
        ScContinuationPolicy::ForcePassthrough => ResponsesContinuationPolicy::ForcePassthrough,
        ScContinuationPolicy::ForceReplay => ResponsesContinuationPolicy::ForceReplay,
    }
}

pub(super) fn continuation_policy_from_pg(
    policy: ResponsesContinuationPolicy,
) -> ScContinuationPolicy {
    match policy {
        ResponsesContinuationPolicy::ForcePassthrough => ScContinuationPolicy::ForcePassthrough,
        ResponsesContinuationPolicy::ForceReplay => ScContinuationPolicy::ForceReplay,
    }
}

pub(super) fn routing_strategy_from_pg(strategy: ModelRouteRoutingStrategy) -> ScRoutingStrategy {
    match strategy {
        ModelRouteRoutingStrategy::ClientKeyRendezvous => ScRoutingStrategy::ClientKeyRendezvous,
        ModelRouteRoutingStrategy::ResponsesSessionAffinity => {
            ScRoutingStrategy::ResponsesSessionAffinity
        }
    }
}

pub(super) fn scope_from_string(scope: &str) -> Result<ScRouteScope> {
    match scope {
        "admin" => Ok(ScRouteScope::Admin),
        "user" => Ok(ScRouteScope::User),
        other => anyhow::bail!("unknown scope {other:?}"),
    }
}

pub(super) fn from_sqlite<F>(route: ScModelRoute, endpoint_lookup: &F) -> UnifiedModelRoute
where
    F: Fn(Uuid) -> (String, bool),
{
    let targets = route
        .targets
        .into_iter()
        .map(|target| {
            let (endpoint_name, endpoint_enabled) = endpoint_lookup(target.endpoint_id);
            UnifiedModelRouteTarget {
                target_id: target.target_id,
                rule_id: route.rule_id,
                endpoint_id: target.endpoint_id,
                endpoint_name: Some(endpoint_name),
                endpoint_enabled,
                position: target.position,
                enabled: target.enabled,
                upstream_model: target.upstream_model,
                responses_continuation_policy: continuation_policy_to_pg(
                    target.responses_continuation_policy,
                ),
            }
        })
        .collect();
    UnifiedModelRoute {
        rule_id: route.rule_id,
        scope: scope_to_string(route.scope),
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern,
        routing_strategy: routing_strategy_to_pg(route.routing_strategy),
        daily_max_requests: route.daily_max_requests,
        monthly_max_requests: route.monthly_max_requests,
        enabled: route.enabled,
        targets,
    }
}

pub(super) fn sqlite_route_from_create(
    rule_id: Uuid,
    input: ModelEndpointRuleCreate,
) -> Result<crate::standalone_config::ModelRouteConfig> {
    use crate::standalone_config::ModelRouteTargetConfig;
    if input.targets.is_empty() {
        anyhow::bail!("model route requires at least one target");
    }
    let scope = scope_from_string(&input.scope)?;
    if scope == ScRouteScope::User && input.owner_user_id.is_none() {
        anyhow::bail!("user route requires an owner_user_id");
    }
    if scope == ScRouteScope::Admin && input.owner_user_id.is_some() {
        anyhow::bail!("admin route cannot have an owner_user_id");
    }
    let routing_strategy = routing_strategy_from_pg(input.routing_strategy);
    let targets = input
        .targets
        .into_iter()
        .enumerate()
        .map(|(index, target)| {
            Ok(ModelRouteTargetConfig {
                target_id: Uuid::new_v4(),
                endpoint_id: target.endpoint_id,
                position: i32::try_from(index).unwrap_or(i32::MAX),
                enabled: target.enabled,
                upstream_model: target.upstream_model,
                responses_continuation_policy: continuation_policy_from_pg(
                    target.responses_continuation_policy,
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(crate::standalone_config::ModelRouteConfig {
        rule_id,
        scope,
        owner_user_id: input.owner_user_id,
        model_pattern: input.model_pattern,
        routing_strategy,
        daily_max_requests: input.daily_max_requests,
        monthly_max_requests: input.monthly_max_requests,
        enabled: input.enabled,
        targets,
    })
}
