use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};

use super::*;
use crate::db::entities::postgres::model_endpoint_rules;
use crate::db::entities::postgres::model_route_targets;
use crate::db::seaorm::{native_error, postgres_connection};

pub async fn create_model_endpoint_rule(
    pool: &PgPool,
    input: ModelEndpointRuleCreate,
) -> Result<ModelEndpointRule> {
    ensure_model_route_targets(&input)?;
    let legacy_endpoint_id = input.targets[0].endpoint_id;
    let db = postgres_connection(pool);
    let transaction = db.begin().await.map_err(native_error)?;
    let rule_id = model_endpoint_rules::Entity::insert(model_endpoint_rules::ActiveModel {
        scope: Set(input.scope.clone()),
        owner_user_id: Set(input.owner_user_id),
        model_pattern: Set(input.model_pattern.clone()),
        routing_strategy: Set(input.routing_strategy.as_str().to_string()),
        endpoint_id: Set(legacy_endpoint_id),
        priority: Set(0),
        enabled: Set(input.enabled),
        ..Default::default()
    })
    .exec(&transaction)
    .await
    .map_err(native_error)?
    .last_insert_id;
    sync_model_route_targets(&transaction, rule_id, &input).await?;
    transaction.commit().await.map_err(native_error)?;
    queries::get_model_endpoint_rule(pool, rule_id)
        .await?
        .ok_or_else(|| anyhow!("model route not found after insert"))
}

pub async fn update_model_endpoint_rule(
    pool: &PgPool,
    rule_id: uuid::Uuid,
    input: ModelEndpointRuleCreate,
) -> Result<Option<ModelEndpointRule>> {
    ensure_model_route_targets(&input)?;
    let legacy_endpoint_id = input.targets[0].endpoint_id;
    let db = postgres_connection(pool);
    let transaction = db.begin().await.map_err(native_error)?;
    let result = model_endpoint_rules::Entity::update_many()
        .filter(model_endpoint_rules::Column::RuleId.eq(rule_id))
        .col_expr(
            model_endpoint_rules::Column::Scope,
            Expr::value(input.scope.clone()),
        )
        .col_expr(
            model_endpoint_rules::Column::OwnerUserId,
            Expr::value(input.owner_user_id),
        )
        .col_expr(
            model_endpoint_rules::Column::ModelPattern,
            Expr::value(input.model_pattern.clone()),
        )
        .col_expr(
            model_endpoint_rules::Column::RoutingStrategy,
            Expr::value(input.routing_strategy.as_str().to_string()),
        )
        .col_expr(
            model_endpoint_rules::Column::EndpointId,
            Expr::value(legacy_endpoint_id),
        )
        .col_expr(model_endpoint_rules::Column::Priority, Expr::value(0))
        .col_expr(
            model_endpoint_rules::Column::Enabled,
            Expr::value(input.enabled),
        )
        .col_expr(
            model_endpoint_rules::Column::UpdatedAt,
            Expr::current_timestamp(),
        )
        .exec(&transaction)
        .await
        .map_err(native_error)?;
    if result.rows_affected == 0 {
        transaction.rollback().await.map_err(native_error)?;
        return Ok(None);
    }
    sync_model_route_targets(&transaction, rule_id, &input).await?;
    transaction.commit().await.map_err(native_error)?;
    queries::get_model_endpoint_rule(pool, rule_id).await
}

pub async fn delete_model_endpoint_rule(pool: &PgPool, rule_id: uuid::Uuid) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = model_endpoint_rules::Entity::delete_many()
        .filter(model_endpoint_rules::Column::RuleId.eq(rule_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

async fn sync_model_route_targets(
    transaction: &sea_orm::DatabaseTransaction,
    rule_id: uuid::Uuid,
    input: &ModelEndpointRuleCreate,
) -> Result<()> {
    model_route_targets::Entity::delete_many()
        .filter(model_route_targets::Column::RuleId.eq(rule_id))
        .exec(transaction)
        .await
        .map_err(native_error)?;

    for (position, target) in input.targets.iter().enumerate() {
        let proxy_override = target
            .proxy_url_override
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        // Issue #378 Phase I: `active_windows` already normalized by the
        // admin layer (`None`/empty means all-day -> NULL).
        let active_windows = match target.active_windows.as_deref() {
            None | Some([]) => None,
            Some(windows) => active_windows::storage_value(windows),
        };
        // Issue #637: trim the free-form override; blank means inherit.
        let service_tier = crate::db::normalize_service_tier(target.service_tier.as_deref());
        model_route_targets::Entity::insert(model_route_targets::ActiveModel {
            rule_id: Set(rule_id),
            endpoint_id: Set(target.endpoint_id),
            position: Set(position as i32),
            enabled: Set(target.enabled),
            upstream_model: Set(target.upstream_model.clone()),
            native_api: Set(target.native_api.as_str().to_string()),
            proxy_url_override: Set(proxy_override.map(str::to_string)),
            active_windows: Set(active_windows),
            dev_system_normalize: Set(target.dev_system_normalize),
            thinking_downgrade_enabled: Set(target.thinking_downgrade_enabled),
            thinking_effort_override: Set(target.thinking_effort_override.clone()),
            compact_mode: Set(target.compact_mode.as_str().to_string()),
            service_tier: Set(service_tier),
            // Issue #1032: nullable positive dimension override; `None`
            // inherits the caller's dimensions.
            embedding_dimensions_override: Set(target.embedding_dimensions_override),
            // Manual new-session preference.
            prefer_new_sessions: Set(target.prefer_new_sessions),
            ..Default::default()
        })
        .exec(transaction)
        .await
        .map_err(native_error)?;
    }
    Ok(())
}

fn ensure_model_route_targets(input: &ModelEndpointRuleCreate) -> Result<()> {
    if input.targets.is_empty() {
        return Err(anyhow!("model route requires at least one target"));
    }
    Ok(())
}
