use super::*;
use chrono::Duration as ChronoDuration;
use std::time::Duration;

const RAW_REQUEST_PRUNE_BATCH_SIZE: i64 = 500;
const RAW_REQUEST_PRUNE_LOCK_KEY: i64 = 0x7072_756e_6552_6177;

#[derive(Debug)]
struct RawPayloadPruneBatch {
    deleted_count: Option<i64>,
    cleared_count: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawPayloadMaintenanceReport {
    pub partitions_created: u64,
    pub raw_rows_deleted: u64,
    pub partitions_dropped: u64,
}

pub async fn prune_usage_events(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query_file!("src/sql/usage/prune_usage_events.sql", retention_days)
        .execute(&mut *tx)
        .await?;
    cleanup_orphan_usage_prompt_blocks(&mut tx).await?;
    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn run_raw_payload_maintenance(
    pool: &PgPool,
    retention_days: i64,
) -> Result<Option<RawPayloadMaintenanceReport>> {
    let Some(mut conn) = try_acquire_raw_request_prune_lock(pool).await? else {
        return Ok(None);
    };

    let result = run_raw_payload_maintenance_locked(&mut conn, retention_days).await;
    let released = sqlx::query_file_scalar!(
        "src/sql/usage/release_raw_request_prune_lock.sql",
        RAW_REQUEST_PRUNE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await;

    match (result, released) {
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(unlock_error)) => {
            tracing::error!(error = %unlock_error, "failed to release raw request prune advisory lock");
            Err(error)
        }
        (Ok(report), Ok(Some(true))) => Ok(Some(report)),
        (Ok(_), Ok(_)) => Err(anyhow::anyhow!(
            "raw request prune advisory lock was not held during release"
        )),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

async fn run_raw_payload_maintenance_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    retention_days: i64,
) -> Result<RawPayloadMaintenanceReport> {
    let now = Utc::now();
    let prune_cutoff = now - ChronoDuration::days(retention_days);
    let partial_partition_start = prune_cutoff
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
        .ok_or_else(|| anyhow::anyhow!("invalid raw payload retention cutoff"))?;
    let partitions_created =
        crate::db::usage::raw_partitions::ensure_raw_payload_partitions(conn, now).await?;
    sqlx::query_file!(
        "src/sql/usage/clear_expired_raw_payload_metadata.sql",
        prune_cutoff,
    )
    .execute(&mut **conn)
    .await?;
    let raw_rows_deleted =
        prune_raw_payload_batches(conn, prune_cutoff, partial_partition_start).await?;
    let partitions_dropped = crate::db::usage::raw_partitions::drop_expired_raw_payload_partitions(
        conn,
        now - ChronoDuration::days(retention_days.max(1)),
    )
    .await?;
    sqlx::query_file!("src/sql/usage/vacuum_request_records.sql")
        .execute(&mut **conn)
        .await?;
    sqlx::query_file!("src/sql/usage/vacuum_raw_payloads.sql")
        .execute(&mut **conn)
        .await?;

    Ok(RawPayloadMaintenanceReport {
        partitions_created,
        raw_rows_deleted,
        partitions_dropped,
    })
}

async fn prune_raw_payload_batches(
    connection: &mut sqlx::postgres::PgConnection,
    cutoff: DateTime<Utc>,
    partial_partition_start: DateTime<Utc>,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let batch = sqlx::query_file_as!(
            RawPayloadPruneBatch,
            "src/sql/usage/prune_usage_raw_requests_batch.sql",
            cutoff,
            RAW_REQUEST_PRUNE_BATCH_SIZE,
            partial_partition_start,
        )
        .fetch_one(&mut *connection)
        .await?;
        let deleted_count = batch.deleted_count.unwrap_or_default();
        let cleared_count = batch.cleared_count.unwrap_or_default();
        total += deleted_count.max(0) as u64;
        if deleted_count == 0 {
            break;
        }
        if cleared_count != deleted_count {
            tracing::warn!(
                deleted = deleted_count,
                cleared = cleared_count,
                "raw payload cleanup deleted rows without clearing all main-record metadata"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(total)
}

pub async fn clear_usage_events(
    pool: &PgPool,
    query: RequestRecordClearQuery,
) -> Result<(u64, u64)> {
    let mut tx = pool.begin().await?;
    let result = match query.scope {
        UsageClearScope::CurrentUser => {
            sqlx::query_file!(
                "src/sql/usage/clear_usage_events_current_user.sql",
                query.visible_user_id,
                query.start_at,
                query.end_at,
            )
            .execute(&mut *tx)
            .await?
        }
        UsageClearScope::AllUsers => {
            sqlx::query_file!(
                "src/sql/usage/clear_usage_events_all_users.sql",
                query.start_at,
                query.end_at,
            )
            .execute(&mut *tx)
            .await?
        }
        UsageClearScope::TargetUser => {
            sqlx::query_file!(
                "src/sql/usage/clear_usage_events_target_user.sql",
                query.target_user_id,
                query.start_at,
                query.end_at,
            )
            .execute(&mut *tx)
            .await?
        }
    };
    let deleted_prompt_blocks = cleanup_orphan_usage_prompt_blocks(&mut tx).await?;
    tx.commit().await?;
    Ok((result.rows_affected(), deleted_prompt_blocks))
}

async fn cleanup_orphan_usage_prompt_blocks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_orphan_usage_prompt_blocks.sql")
        .execute(&mut **tx)
        .await?;
    Ok(deleted.rows_affected())
}

async fn try_acquire_raw_request_prune_lock(
    pool: &PgPool,
) -> Result<Option<sqlx::pool::PoolConnection<sqlx::Postgres>>> {
    let mut conn = pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "src/sql/usage/try_acquire_raw_request_prune_lock.sql",
        RAW_REQUEST_PRUNE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await?
    .unwrap_or(false);
    if acquired { Ok(Some(conn)) } else { Ok(None) }
}
