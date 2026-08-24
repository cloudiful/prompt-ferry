use super::*;
use crate::raw_payload_store::RawPayloadStore;
use chrono::Duration as ChronoDuration;
use std::time::Duration;

const RAW_REQUEST_PRUNE_BATCH_SIZE: i64 = 500;
const RAW_REQUEST_PRUNE_LOCK_KEY: i64 = 0x7072_756e_6552_6177;
const RAW_OBJECT_DELETE_BATCH_SIZE: i64 = 200;

#[derive(Debug)]
struct RawPayloadPruneBatch {
    deleted_count: Option<i64>,
    cleared_count: Option<i64>,
}

#[derive(Debug)]
struct RawExpiredObjectRow {
    event_id: i64,
    created_at: DateTime<Utc>,
    raw_object_key: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawPayloadMaintenanceReport {
    pub partitions_created: u64,
    pub raw_rows_deleted: u64,
    pub partitions_dropped: u64,
}

pub async fn run_raw_payload_maintenance(
    pool: &PgPool,
    retention_days: i64,
) -> Result<Option<RawPayloadMaintenanceReport>> {
    run_raw_payload_maintenance_with_store(pool, retention_days, None).await
}

pub(crate) async fn run_raw_payload_maintenance_with_store(
    pool: &PgPool,
    retention_days: i64,
    raw_store: Option<&RawPayloadStore>,
) -> Result<Option<RawPayloadMaintenanceReport>> {
    let Some(mut conn) = try_acquire_raw_request_prune_lock(pool).await? else {
        return Ok(None);
    };

    let result = run_raw_payload_maintenance_locked(&mut conn, retention_days, raw_store).await;
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
    raw_store: Option<&RawPayloadStore>,
) -> Result<RawPayloadMaintenanceReport> {
    let now = Utc::now();
    let prune_cutoff = now - ChronoDuration::days(retention_days);
    let partial_partition_start = prune_cutoff
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
        .ok_or_else(|| anyhow::anyhow!("invalid raw payload retention cutoff"))?;
    // Expired objects are removed from the store before their metadata rows
    // disappear so a crash cannot orphan an object without its expiry record.
    if let Some(raw_store) = raw_store {
        delete_expired_raw_objects(conn, raw_store, prune_cutoff).await;
    }
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

/// Delete expired raw payload objects from the selected store, walking the
/// expired metadata rows in stable key order. Failures are logged and skipped
/// so retention still removes the expired metadata; the orphaned object is
/// left for the store's own lifecycle tooling.
async fn delete_expired_raw_objects(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    raw_store: &RawPayloadStore,
    cutoff: DateTime<Utc>,
) {
    let mut cursor_created_at = DateTime::<Utc>::MIN_UTC;
    let mut cursor_event_id = i64::MIN;
    loop {
        let rows = match sqlx::query_file_as!(
            RawExpiredObjectRow,
            "src/sql/usage/list_expired_raw_payload_objects.sql",
            cutoff,
            cursor_created_at,
            cursor_event_id,
            RAW_OBJECT_DELETE_BATCH_SIZE,
        )
        .fetch_all(&mut **conn)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(error = %error, "failed to list expired raw payload objects");
                return;
            }
        };
        if rows.is_empty() {
            return;
        }
        for row in rows {
            if let Err(error) = raw_store.delete(&row.raw_object_key).await {
                tracing::warn!(
                    error = %error,
                    event_id = row.event_id,
                    object_key = %row.raw_object_key,
                    "failed to delete expired raw payload object"
                );
            }
            cursor_created_at = row.created_at;
            cursor_event_id = row.event_id;
        }
    }
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
