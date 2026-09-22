//! Mapping helpers between the unified model route DTOs and the
//! PostgreSQL/SQLite row models.

use anyhow::Result;
use uuid::Uuid;

use crate::db::{
    ModelEndpointRule as PgModelEndpointRule, ModelEndpointRuleCreate, ModelRouteRoutingStrategy,
};
use crate::standalone_config::{
    ModelRouteConfig as ScModelRoute, RouteScope as ScRouteScope,
    RoutingStrategy as ScRoutingStrategy,
};

use super::{UnifiedModelRoute, UnifiedModelRouteTarget};

pub(super) fn from_postgres(rule: PgModelEndpointRule) -> UnifiedModelRoute {
    UnifiedModelRoute {
        rule_id: rule.rule_id,
        scope: rule.scope,
        owner_user_id: rule.owner_user_id,
        model_pattern: rule.model_pattern,
        routing_strategy: rule.routing_strategy,
        enabled: rule.enabled,
        targets: rule.targets.into_iter().map(from_postgres_target).collect(),
    }
}

pub(super) fn from_postgres_target(target: crate::db::ModelRouteTarget) -> UnifiedModelRouteTarget {
    // Issue #368 Phase C (P2): carry the saved-override indicator; the
    // secret itself stays redacted.
    let has_proxy_url_override = target.has_proxy_url_override
        || target
            .proxy_url_override
            .as_deref()
            .is_some_and(|raw| !raw.trim().is_empty());
    UnifiedModelRouteTarget {
        target_id: target.target_id,
        rule_id: target.rule_id,
        endpoint_id: target.endpoint_id,
        endpoint_name: target.endpoint_name,
        endpoint_enabled: target.endpoint_enabled,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model,
        native_api: target.native_api,
        has_proxy_url_override,
        active_windows: target.active_windows,
        dev_system_normalize: target.dev_system_normalize,
        // Issue #566: carry the thinking adaptation switch.
        thinking_downgrade_enabled: target.thinking_downgrade_enabled,
        thinking_effort_override: target.thinking_effort_override,
        compact_mode: target.compact_mode,
    }
}

pub(super) fn scope_to_string(scope: ScRouteScope) -> String {
    match scope {
        ScRouteScope::Admin => "admin".to_string(),
        ScRouteScope::User => "user".to_string(),
    }
}

pub(super) fn routing_strategy_to_pg(strategy: ScRoutingStrategy) -> ModelRouteRoutingStrategy {
    // Issue #466: only `responses_session_affinity` remains.
    match strategy {
        ScRoutingStrategy::ResponsesSessionAffinity => {
            ModelRouteRoutingStrategy::ResponsesSessionAffinity
        }
    }
}

pub(super) fn routing_strategy_from_pg(strategy: ModelRouteRoutingStrategy) -> ScRoutingStrategy {
    // Issue #466: only `responses_session_affinity` remains.
    match strategy {
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
            // Issue #368 Phase C (P2): SQLite 0018 envelope already
            // decrypted by the store; presence means saved.
            let has_proxy_url_override = target
                .proxy_url_override
                .as_deref()
                .is_some_and(|raw| !raw.trim().is_empty());
            // Issue #378 Phase I: 0020 plaintext schedule; corrupt reads as
            // all-day for display (routing fails closed separately).
            let active_windows = crate::db::parse_stored_windows(target.active_windows.as_deref())
                .unwrap_or_default();
            UnifiedModelRouteTarget {
                target_id: target.target_id,
                rule_id: route.rule_id,
                endpoint_id: target.endpoint_id,
                endpoint_name: Some(endpoint_name),
                endpoint_enabled,
                position: target.position,
                enabled: target.enabled,
                upstream_model: target.upstream_model,
                native_api: target.native_api,
                has_proxy_url_override,
                active_windows,
                dev_system_normalize: target.dev_system_normalize,
                // Issue #566: 0030 INTEGER 0/1; missing reads as disabled.
                thinking_downgrade_enabled: target.thinking_downgrade_enabled,
                thinking_effort_override: target.thinking_effort_override,
                // Issue #502 Task 5: 0028 plaintext compact mode; unknown
                // reads as `passthrough`.
                compact_mode: crate::db::CompactMode::parse(&target.compact_mode),
            }
        })
        .collect();
    UnifiedModelRoute {
        rule_id: route.rule_id,
        scope: scope_to_string(route.scope),
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern,
        routing_strategy: routing_strategy_to_pg(route.routing_strategy),
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
            let proxy_url_override = target
                .proxy_url_override
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            // Issue #378 Phase I: already normalized by the admin layer;
            // `None`/empty means all-day -> NULL.
            let active_windows = match target.active_windows.as_deref() {
                None | Some([]) => None,
                Some(windows) => crate::db::storage_value(windows),
            };
            Ok(ModelRouteTargetConfig {
                target_id: Uuid::new_v4(),
                endpoint_id: target.endpoint_id,
                position: i32::try_from(index).unwrap_or(i32::MAX),
                enabled: target.enabled,
                upstream_model: target.upstream_model,
                // Issue #409 Phase 1: target port type, default `Auto`.
                native_api: target.native_api,
                proxy_url_override,
                active_windows,
                // Issue #392 Phase K: always sent (no omit); direct carry.
                dev_system_normalize: target.dev_system_normalize,
                // Issue #566: always sent (no omit); direct carry.
                thinking_downgrade_enabled: target.thinking_downgrade_enabled,
                // Issue #464: always sent (`None`/empty means inherit);
                // trim/empty normalizes to `None` (the admin layer already
                // allowlist-validates; storage keeps the trimmed value).
                thinking_effort_override: target
                    .thinking_effort_override
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty()),
                // Issue #502 Task 5: always sent; `passthrough` default.
                compact_mode: target.compact_mode.as_str().to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(crate::standalone_config::ModelRouteConfig {
        rule_id,
        scope,
        owner_user_id: input.owner_user_id,
        model_pattern: input.model_pattern,
        routing_strategy,
        enabled: input.enabled,
        targets,
    })
}
