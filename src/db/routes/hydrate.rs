use sea_orm::sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query, SelectStatement};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use super::*;
use crate::db::entities::postgres::model_endpoint_rules;
use crate::db::entities::postgres::model_route_targets;

/// The endpoint table stays outside the migrated entities, so the route
/// joins address it by name.
fn endpoint_table() -> Alias {
    Alias::new("provider_endpoints")
}

fn endpoint_column(name: &str) -> Alias {
    Alias::new(name)
}

pub(super) async fn hydrate_rules(
    connection: &DatabaseConnection,
    rows: Vec<model_endpoint_rules::Model>,
) -> Result<Vec<ModelEndpointRule>> {
    let rule_ids = rows.iter().map(|row| row.rule_id).collect::<Vec<_>>();
    let targets = load_targets(connection, &rule_ids).await?;
    Ok(rows
        .into_iter()
        .map(|row| ModelEndpointRule {
            rule_id: row.rule_id,
            scope: row.scope,
            owner_user_id: row.owner_user_id,
            model_pattern: row.model_pattern,
            routing_strategy: parse_routing_strategy(&row.routing_strategy),
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
            targets: targets
                .iter()
                .filter(|target| target.rule_id == row.rule_id)
                .cloned()
                .collect(),
        })
        .collect())
}

/// Reads the targets of the given rules with their endpoint projection.
async fn load_targets(
    connection: &DatabaseConnection,
    rule_ids: &[uuid::Uuid],
) -> Result<Vec<ModelRouteTarget>> {
    if rule_ids.is_empty() {
        return Ok(Vec::new());
    }
    let statement = target_statement()
        .and_where(
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::RuleId,
            ))
            .is_in(rule_ids.to_vec()),
        )
        .order_by_expr(
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::RuleId,
            )),
            Order::Asc,
        )
        .order_by_expr(
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::Position,
            )),
            Order::Asc,
        )
        .order_by_expr(
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::CreatedAt,
            )),
            Order::Asc,
        )
        .to_owned();
    let rows = connection
        .query_all(&statement)
        .await
        .map_err(crate::db::seaorm::native_error)?;
    rows.iter().map(decode_target).collect()
}

/// The native target projection with its endpoint join.
fn target_statement() -> SelectStatement {
    let targets = model_route_targets::Entity;
    let endpoints = endpoint_table();
    Query::select()
        .expr_as(
            Expr::col((targets, model_route_targets::Column::TargetId)),
            Alias::new("target_id"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::RuleId)),
            Alias::new("rule_id"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::EndpointId)),
            Alias::new("endpoint_id"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("name"))),
            Alias::new("endpoint_name"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("enabled"))),
            Alias::new("endpoint_enabled"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::Position)),
            Alias::new("position"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::Enabled)),
            Alias::new("enabled"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::UpstreamModel)),
            Alias::new("upstream_model"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::NativeApi)),
            Alias::new("native_api"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ProxyUrlOverride)),
            Alias::new("proxy_url_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ActiveWindows)),
            Alias::new("active_windows"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::DevSystemNormalize)),
            Alias::new("dev_system_normalize"),
        )
        .expr_as(
            Expr::col((
                targets,
                model_route_targets::Column::ThinkingDowngradeEnabled,
            )),
            Alias::new("thinking_downgrade_enabled"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ThinkingEffortOverride)),
            Alias::new("thinking_effort_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::CompactMode)),
            Alias::new("compact_mode"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ServiceTier)),
            Alias::new("service_tier"),
        )
        .expr_as(
            Expr::col((
                targets,
                model_route_targets::Column::EmbeddingDimensionsOverride,
            )),
            Alias::new("embedding_dimensions_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::PreferNewSessions)),
            Alias::new("prefer_new_sessions"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::CreatedAt)),
            Alias::new("created_at"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::UpdatedAt)),
            Alias::new("updated_at"),
        )
        .from(targets)
        .join(
            JoinType::LeftJoin,
            endpoints.clone(),
            Expr::col((endpoints.clone(), endpoint_column("endpoint_id"))).eq(Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::EndpointId,
            ))),
        )
        .to_owned()
}

fn decode_target(result: &sea_orm::QueryResult) -> Result<ModelRouteTarget> {
    let row = ModelRouteTargetRow {
        target_id: result.try_get("", "target_id")?,
        rule_id: result.try_get("", "rule_id")?,
        endpoint_id: result.try_get("", "endpoint_id")?,
        endpoint_name: result.try_get("", "endpoint_name")?,
        endpoint_enabled: result
            .try_get::<Option<bool>>("", "endpoint_enabled")?
            .unwrap_or(false),
        position: result.try_get("", "position")?,
        enabled: result.try_get("", "enabled")?,
        upstream_model: result.try_get("", "upstream_model")?,
        native_api: result.try_get("", "native_api")?,
        proxy_url_override: result.try_get("", "proxy_url_override")?,
        active_windows: result.try_get("", "active_windows")?,
        dev_system_normalize: result.try_get("", "dev_system_normalize")?,
        thinking_downgrade_enabled: result.try_get("", "thinking_downgrade_enabled")?,
        thinking_effort_override: result.try_get("", "thinking_effort_override")?,
        compact_mode: result.try_get("", "compact_mode")?,
        service_tier: result.try_get("", "service_tier")?,
        embedding_dimensions_override: result.try_get("", "embedding_dimensions_override")?,
        prefer_new_sessions: result.try_get("", "prefer_new_sessions")?,
        created_at: result.try_get("", "created_at")?,
        updated_at: result.try_get("", "updated_at")?,
    };
    // Issue #368 Phase C (P2): derive the saved-override indicator;
    // the secret itself stays `skip_serializing`.
    let has_proxy_url_override = row
        .proxy_url_override
        .as_deref()
        .is_some_and(|raw| !raw.trim().is_empty());
    // Issue #378 Phase I: stored JSON array; corrupt reads as
    // all-day for display (routing fails closed separately).
    let active_windows =
        active_windows::parse_stored_windows(row.active_windows.as_deref()).unwrap_or_default();
    // Issue #409 Phase 1: per-target native API, default `auto`
    // (the column default already covers pre-migration rows).
    let native_api = row
        .native_api
        .as_deref()
        .map(parse_native_api)
        .unwrap_or(NativeApi::Auto);
    Ok(ModelRouteTarget {
        target_id: row.target_id,
        rule_id: row.rule_id,
        endpoint_id: row.endpoint_id,
        endpoint_name: row.endpoint_name,
        endpoint_enabled: row.endpoint_enabled,
        position: row.position,
        enabled: row.enabled,
        upstream_model: row.upstream_model,
        native_api,
        proxy_url_override: row.proxy_url_override,
        has_proxy_url_override,
        active_windows,
        // Issue #392 Phase K: normalize switch, default-off for
        // pre-migration rows (the column default already covers).
        dev_system_normalize: row.dev_system_normalize,
        // Issue #566: thinking adaptation switch, default-off for
        // pre-migration rows (the column default already covers).
        thinking_downgrade_enabled: row.thinking_downgrade_enabled,
        // Issue #464: per-target thinking effort override; None
        // means inherit (follow the caller).
        thinking_effort_override: row.thinking_effort_override,
        // Issue #502 Task 5: per-target compact mode; missing
        // reads as `passthrough` (no new default semantics).
        compact_mode: crate::db::resolve_target_compact_mode(row.compact_mode.as_deref()),
        // Issue #637: free-form per-target override; blank means
        // inherit (NULL).
        service_tier: crate::db::normalize_service_tier(row.service_tier.as_deref()),
        // Issue #1032: nullable positive dimension override; `None`
        // inherits the caller's dimensions.
        embedding_dimensions_override: row.embedding_dimensions_override,
        // Manual new-session preference, default off.
        prefer_new_sessions: row.prefer_new_sessions,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub(super) async fn model_route_candidates_by_rule(
    pool: &PgPool,
    rule_id: Option<uuid::Uuid>,
    user_id: Option<i64>,
) -> Result<Vec<ModelRouteCandidate>> {
    let connection = crate::db::seaorm::postgres_connection(pool);
    let user_id = if rule_id.is_some() {
        None
    } else {
        Some(user_id.ok_or_else(|| anyhow!("user_id is required"))?)
    };
    let mut statement = candidate_statement();
    let statement = match rule_id {
        Some(rule_id) => statement.and_where(
            Expr::col((
                model_endpoint_rules::Entity,
                model_endpoint_rules::Column::RuleId,
            ))
            .eq(rule_id),
        ),
        None => {
            let user_id = user_id.ok_or_else(|| anyhow!("user_id is required"))?;
            statement.and_where(
                Expr::col((
                    model_endpoint_rules::Entity,
                    model_endpoint_rules::Column::Scope,
                ))
                .eq("user")
                .and(
                    Expr::col((
                        model_endpoint_rules::Entity,
                        model_endpoint_rules::Column::OwnerUserId,
                    ))
                    .eq(user_id),
                )
                .or(Expr::col((
                    model_endpoint_rules::Entity,
                    model_endpoint_rules::Column::Scope,
                ))
                .eq("admin")
                .and(
                    Expr::col((
                        model_endpoint_rules::Entity,
                        model_endpoint_rules::Column::OwnerUserId,
                    ))
                    .is_null(),
                )),
            )
        }
    };
    let statement = statement
        .order_by_expr(
            Expr::cust("CASE WHEN model_endpoint_rules.scope = 'user' THEN 0 ELSE 1 END"),
            Order::Asc,
        )
        .order_by_expr(
            Expr::col((
                model_endpoint_rules::Entity,
                model_endpoint_rules::Column::UpdatedAt,
            )),
            Order::Desc,
        )
        .order_by_expr(
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::Position,
            )),
            Order::Asc,
        )
        .to_owned();
    let rows = connection
        .query_all(&statement)
        .await
        .map_err(crate::db::seaorm::native_error)?;
    let rows = rows
        .iter()
        .map(decode_candidate)
        .collect::<Result<Vec<_>>>()?;
    let endpoint_api_keys = crate::db::endpoints::list_endpoint_api_keys_by_endpoint_id(
        pool,
        &rows.iter().map(|row| row.endpoint_id).collect::<Vec<_>>(),
    )
    .await?;
    // Issue #1004 P1: one existence query hydrates the non-secret
    // subscription eligibility flag for every candidate target.
    let subscription_endpoints = crate::db::endpoints::list_oauth_token_endpoint_ids(pool).await?;

    let mut grouped = Vec::<ModelRouteCandidate>::new();
    for row in rows {
        let api_keys = endpoint_api_keys
            .get(&row.endpoint_id)
            .cloned()
            .filter(|keys| !keys.is_empty())
            .unwrap_or_else(|| {
                fallback_api_keys(row.endpoint_id, &row.endpoint_name, &row.api_key)
            });
        let provider = crate::db::EndpointProvider::from_str_or_default(&row.provider);
        // Issue #637: target override wins, else the endpoint value, else
        // inherit (both blank/NULL).
        let service_tier = crate::db::resolve_service_tier(
            row.endpoint_service_tier.as_deref(),
            row.target_service_tier.as_deref(),
        );
        // Issue #409 Phase 1: target explicit wins, else endpoint fallback.
        // Both `Auto` stays `Auto` for per-caller `resolve_auto_protocol`.
        let target_native_api = row
            .target_native_api
            .as_deref()
            .map(parse_native_api)
            .unwrap_or(NativeApi::Auto);
        let endpoint_native_api = parse_native_api(&row.native_api);
        let native_api =
            crate::db::resolve_target_native_api(target_native_api, endpoint_native_api);
        if let Some(candidate) = grouped
            .iter_mut()
            .find(|candidate| candidate.rule_id == row.rule_id)
        {
            candidate.targets.push(ModelRouteCandidateTarget {
                target_id: row.target_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name.clone(),
                base_url: row.base_url.clone(),
                api_key: row.api_key.clone(),
                api_keys,
                key_lb_enabled: row.key_lb_enabled,
                native_api,
                target_native_api,
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model.clone(),
                provider,
                service_tier,
                proxy_url: row.proxy_url.clone(),
                proxy_url_override: row.proxy_url_override.clone(),
                active_windows: row.active_windows.clone(),
                endpoint_active_windows: row.endpoint_active_windows.clone(),
                dev_system_normalize: row.dev_system_normalize,
                thinking_downgrade_enabled: row.thinking_downgrade_enabled,
                thinking_effort_override: row.thinking_effort_override.clone(),
                compact_mode: crate::db::resolve_target_compact_mode(row.compact_mode.as_deref()),
                // Issue #1004 P1: subscription eligibility, hydrated from the
                // token-id existence query.
                has_oauth_token: subscription_endpoints.contains(&row.endpoint_id),
                // Issue #1032: copied target dimension override; `None`
                // inherits the caller's dimensions.
                embedding_dimensions_override: row.embedding_dimensions_override,
                // Per-target new-session preference.
                prefer_new_sessions: row.prefer_new_sessions,
            });
            continue;
        }
        grouped.push(ModelRouteCandidate {
            rule_id: row.rule_id,
            scope: row.scope.clone(),
            owner_user_id: row.owner_user_id,
            model_pattern: row.model_pattern.clone(),
            routing_strategy: parse_routing_strategy(&row.routing_strategy),
            updated_at: row.updated_at,
            targets: vec![ModelRouteCandidateTarget {
                target_id: row.target_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name,
                base_url: row.base_url,
                api_key: row.api_key,
                api_keys,
                key_lb_enabled: row.key_lb_enabled,
                native_api,
                target_native_api,
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model,
                provider,
                service_tier,
                proxy_url: row.proxy_url,
                proxy_url_override: row.proxy_url_override,
                active_windows: row.active_windows,
                endpoint_active_windows: row.endpoint_active_windows,
                dev_system_normalize: row.dev_system_normalize,
                thinking_downgrade_enabled: row.thinking_downgrade_enabled,
                thinking_effort_override: row.thinking_effort_override,
                compact_mode: crate::db::resolve_target_compact_mode(row.compact_mode.as_deref()),
                // Issue #1004 P1: subscription eligibility, hydrated from the
                // token-id existence query.
                has_oauth_token: subscription_endpoints.contains(&row.endpoint_id),
                // Issue #1032: copied target dimension override; `None`
                // inherits the caller's dimensions.
                embedding_dimensions_override: row.embedding_dimensions_override,
                // Per-target new-session preference.
                prefer_new_sessions: row.prefer_new_sessions,
            }],
        });
    }
    Ok(grouped)
}

/// The native candidate projection over rules, targets and endpoints.
fn candidate_statement() -> SelectStatement {
    let rules = model_endpoint_rules::Entity;
    let targets = model_route_targets::Entity;
    let endpoints = endpoint_table();
    Query::select()
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::RuleId)),
            Alias::new("rule_id"),
        )
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::Scope)),
            Alias::new("scope"),
        )
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::OwnerUserId)),
            Alias::new("owner_user_id"),
        )
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::ModelPattern)),
            Alias::new("model_pattern"),
        )
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::RoutingStrategy)),
            Alias::new("routing_strategy"),
        )
        .expr_as(
            Expr::col((rules, model_endpoint_rules::Column::UpdatedAt)),
            Alias::new("updated_at"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::TargetId)),
            Alias::new("target_id"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("endpoint_id"))),
            Alias::new("endpoint_id"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("name"))),
            Alias::new("endpoint_name"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("base_url"))),
            Alias::new("base_url"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("api_key"))),
            Alias::new("api_key"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("proxy_url"))),
            Alias::new("proxy_url"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("key_lb_enabled"))),
            Alias::new("key_lb_enabled"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("native_api"))),
            Alias::new("native_api"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::NativeApi)),
            Alias::new("target_native_api"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("provider"))),
            Alias::new("provider"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("service_tier"))),
            Alias::new("endpoint_service_tier"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ServiceTier)),
            Alias::new("target_service_tier"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::Position)),
            Alias::new("position"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::Enabled)),
            Alias::new("target_enabled"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::UpstreamModel)),
            Alias::new("upstream_model"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ProxyUrlOverride)),
            Alias::new("proxy_url_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ActiveWindows)),
            Alias::new("active_windows"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("active_windows"))),
            Alias::new("endpoint_active_windows"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::DevSystemNormalize)),
            Alias::new("dev_system_normalize"),
        )
        .expr_as(
            Expr::col((
                targets,
                model_route_targets::Column::ThinkingDowngradeEnabled,
            )),
            Alias::new("thinking_downgrade_enabled"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::ThinkingEffortOverride)),
            Alias::new("thinking_effort_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::CompactMode)),
            Alias::new("compact_mode"),
        )
        .expr_as(
            Expr::col((
                targets,
                model_route_targets::Column::EmbeddingDimensionsOverride,
            )),
            Alias::new("embedding_dimensions_override"),
        )
        .expr_as(
            Expr::col((targets, model_route_targets::Column::PreferNewSessions)),
            Alias::new("prefer_new_sessions"),
        )
        .from(rules)
        .join(
            JoinType::InnerJoin,
            targets,
            Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::RuleId,
            ))
            .eq(Expr::col((
                model_endpoint_rules::Entity,
                model_endpoint_rules::Column::RuleId,
            ))),
        )
        .join(
            JoinType::InnerJoin,
            endpoints.clone(),
            Expr::col((endpoints.clone(), endpoint_column("endpoint_id"))).eq(Expr::col((
                model_route_targets::Entity,
                model_route_targets::Column::EndpointId,
            ))),
        )
        .and_where(Expr::col((rules, model_endpoint_rules::Column::Enabled)).eq(true))
        .and_where(Expr::col((targets, model_route_targets::Column::Enabled)).eq(true))
        .and_where(Expr::col((endpoints, endpoint_column("enabled"))).eq(true))
        .to_owned()
}

fn decode_candidate(result: &sea_orm::QueryResult) -> Result<ModelRouteCandidateRow> {
    Ok(ModelRouteCandidateRow {
        rule_id: result.try_get("", "rule_id")?,
        scope: result.try_get("", "scope")?,
        owner_user_id: result.try_get("", "owner_user_id")?,
        model_pattern: result.try_get("", "model_pattern")?,
        routing_strategy: result.try_get("", "routing_strategy")?,
        updated_at: result.try_get("", "updated_at")?,
        target_id: result.try_get("", "target_id")?,
        endpoint_id: result.try_get("", "endpoint_id")?,
        endpoint_name: result.try_get("", "endpoint_name")?,
        base_url: result.try_get("", "base_url")?,
        api_key: result.try_get("", "api_key")?,
        proxy_url: result.try_get("", "proxy_url")?,
        key_lb_enabled: result.try_get("", "key_lb_enabled")?,
        native_api: result.try_get("", "native_api")?,
        target_native_api: result.try_get("", "target_native_api")?,
        provider: result.try_get("", "provider")?,
        endpoint_service_tier: result.try_get("", "endpoint_service_tier")?,
        target_service_tier: result.try_get("", "target_service_tier")?,
        position: result.try_get("", "position")?,
        target_enabled: result.try_get("", "target_enabled")?,
        upstream_model: result.try_get("", "upstream_model")?,
        proxy_url_override: result.try_get("", "proxy_url_override")?,
        active_windows: result.try_get("", "active_windows")?,
        endpoint_active_windows: result.try_get("", "endpoint_active_windows")?,
        dev_system_normalize: result.try_get("", "dev_system_normalize")?,
        thinking_downgrade_enabled: result.try_get("", "thinking_downgrade_enabled")?,
        thinking_effort_override: result.try_get("", "thinking_effort_override")?,
        compact_mode: result.try_get("", "compact_mode")?,
        embedding_dimensions_override: result.try_get("", "embedding_dimensions_override")?,
        prefer_new_sessions: result.try_get("", "prefer_new_sessions")?,
    })
}
