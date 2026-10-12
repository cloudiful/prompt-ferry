//! Reading the API key set of an endpoint, and writing a replacement for it.
//!
//! Both run on the caller's transaction. A save holds the endpoint row's lock
//! before it reads the key set, so the plan is built from the rows its own
//! writes will land on and two saves of one endpoint cannot interleave.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::{Executor, Postgres, Transaction};
use uuid::Uuid;

use crate::db::types::{EndpointApiKey, EndpointApiKeyCreate};

use super::keys_diff::{DiffPlan, RENUMBER_OFFSET, plan_key_persistence};

/// The stored API keys of the given endpoints, grouped by endpoint.
pub(crate) async fn list_endpoint_api_keys_by_endpoint_id<'e, E>(
    executor: E,
    endpoint_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<EndpointApiKey>>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = if endpoint_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_file_as!(
            EndpointApiKey,
            "src/sql/endpoints/list_endpoint_api_keys.sql",
            endpoint_ids,
        )
        .fetch_all(executor)
        .await?
    };
    let mut rows_by_endpoint = HashMap::<Uuid, Vec<EndpointApiKey>>::new();
    for row in rows {
        rows_by_endpoint
            .entry(row.endpoint_id)
            .or_default()
            .push(row);
    }
    Ok(rows_by_endpoint)
}

/// Lock the endpoint row for the rest of the transaction.
///
/// Call this before reading the key set of an endpoint that is being updated.
/// A create does not need it: its own insert already holds the new row.
pub(crate) async fn lock_endpoint_for_update(
    tx: &mut Transaction<'_, Postgres>,
    endpoint_id: Uuid,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/endpoints/lock_endpoint_for_update.sql",
        endpoint_id
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(())
}

/// Make the stored key set of one endpoint match the request.
///
/// The caller must already hold the endpoint row's lock on this transaction.
/// Rows that did not change are not written, so their `updated_at` survives a
/// save that left them alone.
pub(crate) async fn replace_endpoint_api_keys(
    tx: &mut Transaction<'_, Postgres>,
    endpoint_id: Uuid,
    api_keys: &[EndpointApiKeyCreate],
) -> Result<()> {
    let current = list_endpoint_api_keys_by_endpoint_id(&mut **tx, &[endpoint_id])
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
    tx: &mut Transaction<'_, Postgres>,
    endpoint_id: Uuid,
    plan: &DiffPlan,
) -> Result<()> {
    // Deletions first. They free the slots the survivors need, and the foreign
    // keys that point at a removed key are set to NULL by the schema, so a
    // removal and an insertion into the same position do not conflict.
    if !plan.removed.is_empty() {
        sqlx::query_file!(
            "src/sql/endpoints/delete_endpoint_api_keys_except.sql",
            endpoint_id,
            &plan.retained,
        )
        .execute(&mut **tx)
        .await?;
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
        sqlx::query_file!(
            "src/sql/endpoints/renumber_changed_endpoint_api_keys.sql",
            endpoint_id,
            &moving,
            RENUMBER_OFFSET,
        )
        .execute(&mut **tx)
        .await?;
    }

    for insert in &plan.inserts {
        sqlx::query_file!(
            "src/sql/endpoints/insert_endpoint_api_key.sql",
            endpoint_id,
            insert.key_label.as_str(),
            insert.api_key.as_str(),
            insert.position,
            insert.enabled,
        )
        .fetch_one(&mut **tx)
        .await?;
    }

    // Positions go back without touching `updated_at`: reordering a key is not
    // an edit to it. The plan rejects a request whose positions collide, so no
    // two keys are ever sent to the same slot.
    for (key_id, position) in &plan.repositions {
        sqlx::query_file!(
            "src/sql/endpoints/restore_endpoint_api_key_position.sql",
            endpoint_id,
            key_id,
            position,
        )
        .execute(&mut **tx)
        .await?;
    }

    // An edit is what advances `updated_at`; `created_at` is left as it is so a
    // key keeps the age it has had since it was created.
    for rewrite in &plan.rewrites {
        sqlx::query_file!(
            "src/sql/endpoints/update_endpoint_api_key.sql",
            endpoint_id,
            rewrite.key_id,
            rewrite.key_label.as_str(),
            rewrite.api_key.as_str(),
            rewrite.position,
            rewrite.enabled,
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}
