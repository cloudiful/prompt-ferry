use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{
    ChatgptQuotaEndpointActivity, ChatgptQuotaHistoryEndpoint, ChatgptQuotaSnapshot,
    ChatgptQuotaSnapshotSource,
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

pub async fn list_snapshot_history(
    pool: &PgPool,
    endpoint_id: Uuid,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<ChatgptQuotaSnapshot>> {
    let rows = sqlx::query_file_as!(
        SnapshotDbRow,
        "src/sql/quota_snapshots/list_snapshot_history.sql",
        endpoint_id,
        before_id,
        limit.clamp(1, SNAPSHOT_PAGE_MAX),
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(map_snapshot).collect()
}

#[derive(Debug)]
struct QuotaHistoryEndpointDbRow {
    endpoint_id: Uuid,
    provider: String,
}

pub async fn quota_history_endpoint(
    pool: &PgPool,
    endpoint_id: Uuid,
) -> Result<Option<ChatgptQuotaHistoryEndpoint>> {
    Ok(sqlx::query_file_as!(
        QuotaHistoryEndpointDbRow,
        "src/sql/quota_snapshots/quota_history_endpoint.sql",
        endpoint_id,
    )
    .fetch_optional(pool)
    .await?
    .map(|row| ChatgptQuotaHistoryEndpoint {
        endpoint_id: row.endpoint_id,
        provider: row.provider,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        ChatgptQuotaSnapshotSource,
        quota_snapshots::tests::test_support::{
            cleanup_endpoint, create_endpoint, record_snapshot, test_pool,
        },
    };
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn snapshots_page_by_endpoint_and_prune_old_rows_in_bounded_batches() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let now = Utc::now();
        let endpoint_id = create_endpoint(&pool, "openai", true, true).await;
        let other_endpoint = create_endpoint(&pool, "openai", true, true).await;
        let old_id = record_snapshot(
            &pool,
            endpoint_id,
            now - Duration::days(31),
            ChatgptQuotaSnapshotSource::Manual,
        )
        .await;
        let middle_id = record_snapshot(
            &pool,
            endpoint_id,
            now - Duration::days(29),
            ChatgptQuotaSnapshotSource::Request,
        )
        .await;
        let newest_id = record_snapshot(
            &pool,
            endpoint_id,
            now,
            ChatgptQuotaSnapshotSource::Periodic,
        )
        .await;
        let other_id = record_snapshot(
            &pool,
            other_endpoint,
            now,
            ChatgptQuotaSnapshotSource::Request,
        )
        .await;
        let page = list_snapshots(&pool, endpoint_id, None, 2).await.unwrap();
        assert_eq!(
            page.iter().map(|row| row.snapshot_id).collect::<Vec<_>>(),
            vec![newest_id, middle_id]
        );
        let next = list_snapshots(&pool, endpoint_id, Some(middle_id), 2)
            .await
            .unwrap();
        assert_eq!(
            next.iter().map(|row| row.snapshot_id).collect::<Vec<_>>(),
            vec![old_id]
        );
        assert_eq!(
            latest_snapshot(&pool, other_endpoint)
                .await
                .unwrap()
                .unwrap()
                .snapshot_id,
            other_id
        );
        assert_eq!(
            list_snapshots(&pool, other_endpoint, None, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(prune_expired_snapshots(&pool, now, 1).await.unwrap(), 1);
        assert_eq!(
            latest_snapshot(&pool, endpoint_id)
                .await
                .unwrap()
                .unwrap()
                .snapshot_id,
            newest_id
        );
        assert_eq!(
            list_snapshots(&pool, endpoint_id, None, 201)
                .await
                .unwrap()
                .len(),
            2
        );
        cleanup_endpoint(&pool, endpoint_id).await;
        cleanup_endpoint(&pool, other_endpoint).await;
    }

    #[tokio::test]
    async fn eligible_endpoints_and_activity_use_only_ids_and_recent_request_times() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let now = Utc::now();
        let active = create_endpoint(&pool, "openai", true, true).await;
        let idle = create_endpoint(&pool, "openai", true, true).await;
        let disabled = create_endpoint(&pool, "openai", false, true).await;
        let other_provider = create_endpoint(&pool, "generic", true, true).await;
        let missing_oauth = create_endpoint(&pool, "openai", true, false).await;
        for (endpoint_id, created_at) in [
            (active, now - Duration::minutes(5)),
            (idle, now - Duration::minutes(45)),
        ] {
            sqlx::query("INSERT INTO request_records (request_id, endpoint_id, path, created_at) VALUES ($1, $2, '/v1/responses', $3)")
                .bind(Uuid::new_v4())
                .bind(endpoint_id)
                .bind(created_at)
                .execute(&pool)
                .await
                .expect("insert isolated request activity");
        }
        let eligible = eligible_endpoints(&pool).await.unwrap();
        assert!(eligible.contains(&active) && eligible.contains(&idle));
        assert!(!eligible.contains(&disabled));
        assert!(!eligible.contains(&other_provider));
        assert!(!eligible.contains(&missing_oauth));
        let activity = endpoint_activity(&pool, &[active, idle], now - Duration::hours(2))
            .await
            .unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].endpoint_id, active);
        assert!(activity[0].last_activity_at >= now - Duration::minutes(10));
        assert!(
            endpoint_activity(&pool, &[], now - Duration::minutes(30))
                .await
                .unwrap()
                .is_empty()
        );
        for endpoint_id in [active, idle, disabled, other_provider, missing_oauth] {
            cleanup_endpoint(&pool, endpoint_id).await;
        }
    }
}
