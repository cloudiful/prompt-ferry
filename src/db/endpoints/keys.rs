//! Reading the API key set of an endpoint, and writing a replacement for it.
//!
//! Both run on the caller's native connection or transaction. A save holds the
//! endpoint row's lock before it reads the key set, so the plan is built from
//! the rows its own writes will land on and two saves of one endpoint cannot
//! interleave.

use std::collections::HashMap;

use crate::db::{
    entities::postgres::{
        endpoint_api_keys::{Column, Entity},
        provider_endpoints::{Column as EndpointColumn, Entity as EndpointEntity},
    },
    seaorm::{native_error, postgres_connection},
};
use anyhow::Result;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
    sea_query::{Expr, ExprTrait},
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{EndpointApiKey, EndpointApiKeyCreate};

use super::keys_diff::{DiffPlan, RENUMBER_OFFSET, plan_key_persistence};

pub(crate) async fn list_endpoint_api_keys_by_endpoint_id(
    pool: &PgPool,
    endpoint_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<EndpointApiKey>>> {
    list_endpoint_api_keys_native(&postgres_connection(pool), endpoint_ids).await
}

/// The stored API keys of the given endpoints, grouped by endpoint.
pub(crate) async fn list_endpoint_api_keys_native<C>(
    db: &C,
    endpoint_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<EndpointApiKey>>>
where
    C: ConnectionTrait,
{
    let mut grouped = HashMap::<Uuid, Vec<EndpointApiKey>>::new();
    if endpoint_ids.is_empty() {
        return Ok(grouped);
    }
    let rows = Entity::find()
        .filter(Column::EndpointId.is_in(endpoint_ids.iter().copied()))
        .order_by_asc(Column::EndpointId)
        .order_by_asc(Column::Position)
        .order_by_asc(Column::CreatedAt)
        .all(db)
        .await
        .map_err(native_error)?;
    for row in rows {
        grouped
            .entry(row.endpoint_id)
            .or_default()
            .push(EndpointApiKey {
                key_id: row.key_id,
                endpoint_id: row.endpoint_id,
                key_label: row.key_label,
                api_key: row.api_key,
                position: row.position,
                enabled: row.enabled,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
    }
    Ok(grouped)
}

/// Lock the endpoint row for the rest of the transaction.
///
/// Call this before reading the key set of an endpoint that is being updated.
/// A create does not need it: its own insert already holds the new row.
pub(crate) async fn lock_endpoint_for_update(
    tx: &DatabaseTransaction,
    endpoint_id: Uuid,
) -> Result<bool> {
    let locked = EndpointEntity::find()
        .select_only()
        .column(EndpointColumn::EndpointId)
        .filter(EndpointColumn::EndpointId.eq(endpoint_id))
        .lock_exclusive()
        .into_tuple::<Uuid>()
        .one(tx)
        .await
        .map_err(native_error)?;
    Ok(locked.is_some())
}

/// Make the stored key set of one endpoint match the request.
///
/// The caller must already hold the endpoint row's lock on this transaction.
/// Rows that did not change are not written, so their `updated_at` survives a
/// save that left them alone.
pub(crate) async fn replace_endpoint_api_keys(
    tx: &DatabaseTransaction,
    endpoint_id: Uuid,
    api_keys: &[EndpointApiKeyCreate],
) -> Result<()> {
    let current = list_endpoint_api_keys_native(tx, &[endpoint_id])
        .await?
        .remove(&endpoint_id)
        .unwrap_or_default();
    let plan = plan_key_persistence(&current, api_keys)?;
    if plan.is_empty() {
        return Ok(());
    }
    apply_key_plan(tx, endpoint_id, &plan).await
}

/// Issue exactly the statements `plan` calls for.
async fn apply_key_plan(
    tx: &DatabaseTransaction,
    endpoint_id: Uuid,
    plan: &DiffPlan,
) -> Result<()> {
    // Deletions first. They free the slots the survivors need, and the foreign
    // keys that point at a removed key are set to NULL by the schema, so a
    // removal and an insertion into the same position do not conflict.
    if !plan.removed.is_empty() {
        let mut delete = Entity::delete_many().filter(Column::EndpointId.eq(endpoint_id));
        if !plan.retained.is_empty() {
            delete = delete.filter(Column::KeyId.is_not_in(plan.retained.iter().copied()));
        }
        delete.exec(tx).await.map_err(native_error)?;
    }

    // Then shift every moving key out of the way. Only the keys that are moving
    // are touched, so the keys holding their positions are never displaced and
    // the reserved band is the only space used.
    if !plan.repositions.is_empty() {
        let moving = plan
            .repositions
            .iter()
            .map(|(key_id, _)| *key_id)
            .collect::<Vec<_>>();
        Entity::update_many()
            .col_expr(
                Column::Position,
                Expr::col(Column::Position).add(RENUMBER_OFFSET),
            )
            .filter(Column::EndpointId.eq(endpoint_id))
            .filter(Column::KeyId.is_in(moving))
            .exec(tx)
            .await
            .map_err(native_error)?;
    }

    for insert in &plan.inserts {
        Entity::insert(
            crate::db::entities::postgres::endpoint_api_keys::ActiveModel {
                endpoint_id: Set(endpoint_id),
                key_label: Set(insert.key_label.clone()),
                api_key: Set(insert.api_key.clone()),
                position: Set(insert.position),
                enabled: Set(insert.enabled),
                ..Default::default()
            },
        )
        .exec_without_returning(tx)
        .await
        .map_err(native_error)?;
    }

    // Positions go back without touching `updated_at`: reordering a key is not
    // an edit to it. The plan rejects a request whose positions collide, so no
    // two keys are ever sent to the same slot.
    for (key_id, position) in &plan.repositions {
        Entity::update_many()
            .col_expr(Column::Position, Expr::value(*position))
            .filter(Column::EndpointId.eq(endpoint_id))
            .filter(Column::KeyId.eq(*key_id))
            .exec(tx)
            .await
            .map_err(native_error)?;
    }

    // An edit is what advances `updated_at`; `created_at` is left as it is so a
    // key keeps the age it has had since it was created.
    for rewrite in &plan.rewrites {
        Entity::update_many()
            .col_expr(Column::KeyLabel, Expr::value(rewrite.key_label.clone()))
            .col_expr(Column::ApiKey, Expr::value(rewrite.api_key.clone()))
            .col_expr(Column::Position, Expr::value(rewrite.position))
            .col_expr(Column::Enabled, Expr::value(rewrite.enabled))
            .col_expr(Column::UpdatedAt, Expr::current_timestamp())
            .filter(Column::EndpointId.eq(endpoint_id))
            .filter(Column::KeyId.eq(rewrite.key_id))
            .exec(tx)
            .await
            .map_err(native_error)?;
    }

    Ok(())
}
