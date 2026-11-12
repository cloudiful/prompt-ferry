use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{
    ChatgptQuotaEndpointActivity, ChatgptQuotaSnapshot, ChatgptQuotaSnapshotSource,
};

use super::{SNAPSHOT_PAGE_MAX, SNAPSHOT_PRUNE_BATCH_SIZE, SNAPSHOT_RETENTION_DAYS};

#[derive(Debug)]
pub(super) struct SnapshotDbRow {
    pub(super) snapshot_id: i64,
    pub(super) endpoint_id: Uuid,
    pub(super) observed_at: DateTime<Utc>,
    pub(super) plan_type: Option<String>,
    pub(super) limit_reached: Option<bool>,
    pub(super) windows: serde_json::Value,
    pub(super) source: String,
    pub(super) created_at: DateTime<Utc>,
}

pub(super) fn map_snapshot(row: SnapshotDbRow) -> Result<ChatgptQuotaSnapshot> {
    let source = ChatgptQuotaSnapshotSource::from_database(&row.source)
        .context("database contains an unknown ChatGPT quota snapshot source")?;
    Ok(ChatgptQuotaSnapshot {
        snapshot_id: row.snapshot_id,
        endpoint_id: row.endpoint_id,
        observed_at: row.observed_at,
        plan_type: row.plan_type,
        limit_reached: row.limit_reached,
        windows: row.windows,
        source,
        created_at: row.created_at,
    })
}

pub async fn latest_snapshot(
    pool: &PgPool,
    endpoint_id: Uuid,
) -> Result<Option<ChatgptQuotaSnapshot>> {
    sqlx::query_file_as!(
        SnapshotDbRow,
        "src/sql/quota_snapshots/latest_snapshot.sql",
        endpoint_id,
    )
    .fetch_optional(pool)
    .await?
    .map(map_snapshot)
    .transpose()
}

pub async fn list_snapshots(
    pool: &PgPool,
    endpoint_id: Uuid,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<ChatgptQuotaSnapshot>> {
    let rows = sqlx::query_file_as!(
        SnapshotDbRow,
        "src/sql/quota_snapshots/list_snapshots.sql",
        endpoint_id,
        before_id,
        limit.clamp(1, SNAPSHOT_PAGE_MAX),
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(map_snapshot).collect()
}

pub async fn prune_expired_snapshots(
    pool: &PgPool,
    now: DateTime<Utc>,
    batch_size: i64,
) -> Result<u64> {
    let cutoff = now - Duration::days(SNAPSHOT_RETENTION_DAYS);
    let result = sqlx::query_file!(
        "src/sql/quota_snapshots/delete_expired_snapshots.sql",
        cutoff,
        batch_size.clamp(1, SNAPSHOT_PRUNE_BATCH_SIZE),
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn eligible_endpoints(pool: &PgPool) -> Result<Vec<Uuid>> {
    Ok(
        sqlx::query_file_scalar!("src/sql/quota_snapshots/eligible_endpoints.sql",)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn endpoint_activity(
    pool: &PgPool,
    endpoint_ids: &[Uuid],
    since: DateTime<Utc>,
) -> Result<Vec<ChatgptQuotaEndpointActivity>> {
    if endpoint_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_file_as!(
        ChatgptQuotaEndpointActivity,
        "src/sql/quota_snapshots/endpoint_activity.sql",
        endpoint_ids,
        since,
    )
    .fetch_all(pool)
    .await?)
}
