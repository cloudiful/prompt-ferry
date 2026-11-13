use sea_orm::sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query, SelectStatement};
use sea_orm::{ConnectionTrait, EntityTrait, QueryOrder, QuerySelect};

use super::*;
use crate::db::entities::postgres::model_endpoint_rules;
use crate::db::entities::postgres::model_route_targets;
use crate::db::seaorm::{native_error, postgres_connection};

/// The endpoint table stays outside the migrated entities, so the route
/// joins address it by name.
fn endpoint_table() -> Alias {
    Alias::new("provider_endpoints")
}

fn endpoint_column(name: &str) -> Alias {
    Alias::new(name)
}

pub async fn list_model_endpoint_rules(pool: &PgPool) -> Result<Vec<ModelEndpointRule>> {
    let db = postgres_connection(pool);
    let rows = model_endpoint_rules::Entity::find()
        .order_by_asc(Expr::cust("CASE WHEN scope = 'user' THEN 0 ELSE 1 END"))
        .order_by_asc(model_endpoint_rules::Column::OwnerUserId)
        .order_by_asc(model_endpoint_rules::Column::ModelPattern)
        .order_by_desc(model_endpoint_rules::Column::UpdatedAt)
        .all(&db)
        .await
        .map_err(native_error)?;
    hydrate::hydrate_rules(&db, rows).await
}

pub async fn list_model_endpoint_rules_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<ModelRoutePage> {
    let db = postgres_connection(pool);
    let counts = Query::select()
        .expr_as(
            Expr::col(model_endpoint_rules::Column::RuleId).count(),
            Alias::new("total"),
        )
        .from(model_endpoint_rules::Entity)
        .to_owned();
    let count_row = db
        .query_one(&counts)
        .await
        .map_err(native_error)?
        .ok_or_else(|| anyhow!("model route counts returned no row"))?;
    let total: i64 = count_row.try_get("", "total").map_err(native_error)?;
    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let route_rows = model_endpoint_rules::Entity::find()
        .order_by_asc(Expr::cust("CASE WHEN scope = 'user' THEN 0 ELSE 1 END"))
        .order_by_asc(model_endpoint_rules::Column::OwnerUserId)
        .order_by_asc(model_endpoint_rules::Column::ModelPattern)
        .order_by_desc(model_endpoint_rules::Column::UpdatedAt)
        .order_by_asc(model_endpoint_rules::Column::RuleId)
        .offset(first as u64)
        .limit(rows as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(ModelRoutePage {
        total,
        routes: hydrate::hydrate_rules(&db, route_rows).await?,
        first,
        rows,
    })
}

pub async fn get_model_endpoint_rule(
    pool: &PgPool,
    rule_id: uuid::Uuid,
) -> Result<Option<ModelEndpointRule>> {
    let db = postgres_connection(pool);
    let row = model_endpoint_rules::Entity::find_by_id(rule_id)
        .one(&db)
        .await
        .map_err(native_error)?;
    match row {
        Some(row) => Ok(hydrate::hydrate_rules(&db, vec![row])
            .await?
            .into_iter()
            .next()),
        None => Ok(None),
    }
}

pub async fn get_model_route_candidate(
    pool: &PgPool,
    rule_id: uuid::Uuid,
) -> Result<Option<ModelRouteCandidate>> {
    let candidates = hydrate::model_route_candidates_by_rule(pool, Some(rule_id), None).await?;
    Ok(candidates.into_iter().next())
}

pub async fn model_route_candidates(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<ModelRouteCandidate>> {
    hydrate::model_route_candidates_by_rule(pool, None, Some(user_id)).await
}

pub async fn list_visible_model_route_endpoints(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<RouteConfig>> {
    let mut routes = list_visible_model_route_endpoints_strict(pool, user_id).await?;

    if routes.is_empty()
        && let Some(route) = snapshot::effective_route(pool, user_id).await?
    {
        routes.push(route);
    }
    Ok(routes)
}

pub async fn list_visible_model_route_endpoints_strict(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<RouteConfig>> {
    let db = postgres_connection(pool);
    let statement = visible_endpoints_statement(user_id);
    let rows = db.query_all(&statement).await.map_err(native_error)?;
    let routes = rows
        .iter()
        .map(|row| {
            Ok(RouteConfig {
                embedding_dimensions_override: None,
                route_id: row.try_get("", "endpoint_id")?,
                user_id,
                model_route_rule_id: None,
                base_url: row.try_get("", "base_url")?,
                api_key: row.try_get("", "api_key")?,
                endpoint_key_id: None,
                endpoint_key_label: None,
                api_keys: Vec::new(),
                key_lb_enabled: row.try_get("", "key_lb_enabled")?,
                native_api: parse_native_api(&row.try_get::<String>("", "native_api")?),
                upstream_model: None,
                route_selection_reason: crate::db::RouteSelectionReason::Default,
                provider: crate::db::EndpointProvider::from_str_or_default(
                    &row.try_get::<String>("", "provider")?,
                ),
                service_tier: crate::db::normalize_service_tier(
                    row.try_get::<Option<String>>("", "service_tier")?
                        .as_deref(),
                ),
                proxy_url: row.try_get("", "proxy_url")?,
                // Issue #392 Phase K: direct endpoint routes never normalize.
                dev_system_normalize: false,
                // Issue #566: direct routes never enable the thinking adaption.
                thinking_downgrade_enabled: false,
                // Issue #464: direct routes follow the caller.
                thinking_effort_override: None,
                // Issue #502 Task 5: direct routes use passthrough compact.
                compact_mode: crate::db::CompactMode::Passthrough,
                // Issue #1004 P1: stamped from the token-id existence query.
                has_oauth_token: false,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    crate::db::endpoints::attach_route_config_credentials(pool, routes).await
}

/// The native endpoint projection over rules, targets and endpoints.
fn visible_endpoints_statement(user_id: i64) -> SelectStatement {
    let rules = model_endpoint_rules::Entity;
    let targets = model_route_targets::Entity;
    let endpoints = endpoint_table();
    Query::select()
        .distinct_on([(endpoints.clone(), endpoint_column("endpoint_id"))])
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("endpoint_id"))),
            Alias::new("endpoint_id"),
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
            Expr::col((endpoints.clone(), endpoint_column("provider"))),
            Alias::new("provider"),
        )
        .expr_as(
            Expr::col((endpoints.clone(), endpoint_column("service_tier"))),
            Alias::new("service_tier"),
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
        .and_where(Expr::col((endpoints.clone(), endpoint_column("enabled"))).eq(true))
        .and_where(
            Expr::col((rules, model_endpoint_rules::Column::Scope))
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
        .order_by_expr(
            Expr::col((endpoints.clone(), endpoint_column("endpoint_id"))),
            Order::Asc,
        )
        .order_by_expr(
            Expr::cust("CASE WHEN model_endpoint_rules.scope = 'user' THEN 0 ELSE 1 END"),
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
                model_endpoint_rules::Entity,
                model_endpoint_rules::Column::UpdatedAt,
            )),
            Order::Desc,
        )
        .to_owned()
}
