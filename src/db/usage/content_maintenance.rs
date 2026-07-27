use super::*;
use sqlx::Acquire;

const USAGE_CONTENT_MAINTENANCE_BATCH_SIZE: i64 = 500;
const USAGE_CONTENT_MAINTENANCE_LOCK_KEY: i64 = 0x7072_756e_6543_6f6e;

#[derive(Debug)]
struct UsageContentPruneBatch {
    expired_events: i64,
    deleted_block_refs: i64,
    deleted_artifacts: i64,
    deleted_snapshots: i64,
    cleared_tool_arguments: i64,
    deleted_redaction_sessions: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UsageContentMaintenanceReport {
    pub expired_events: u64,
    pub deleted_block_refs: u64,
    pub deleted_artifacts: u64,
    pub deleted_snapshots: u64,
    pub cleared_tool_arguments: u64,
    pub deleted_redaction_sessions: u64,
    pub orphan_prompt_blocks_deleted: u64,
}

pub async fn run_usage_content_maintenance(
    pool: &PgPool,
    retention_days: i64,
) -> Result<Option<UsageContentMaintenanceReport>> {
    let Some(mut conn) = try_acquire_usage_content_maintenance_lock(pool).await? else {
        return Ok(None);
    };

    let result = run_usage_content_maintenance_locked(&mut conn, retention_days).await;
    let released = sqlx::query_file_scalar!(
        "src/sql/usage/release_usage_content_maintenance_lock.sql",
        USAGE_CONTENT_MAINTENANCE_LOCK_KEY,
    )
    .fetch_one(&mut *conn)
    .await;

    match (result, released) {
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(unlock_error)) => {
            tracing::error!(error = %unlock_error, "failed to release usage content maintenance advisory lock");
            Err(error)
        }
        (Ok(report), Ok(true)) => Ok(Some(report)),
        (Ok(_), Ok(false)) => Err(anyhow::anyhow!(
            "usage content maintenance advisory lock was not held during release"
        )),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

async fn run_usage_content_maintenance_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    retention_days: i64,
) -> Result<UsageContentMaintenanceReport> {
    let mut report = UsageContentMaintenanceReport::default();
    loop {
        let mut tx = conn.begin().await?;
        let batch = sqlx::query_file_as!(
            UsageContentPruneBatch,
            "src/sql/usage/prune_usage_content_batch.sql",
            retention_days.max(1),
            USAGE_CONTENT_MAINTENANCE_BATCH_SIZE,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        report.expired_events += batch.expired_events.max(0) as u64;
        report.deleted_block_refs += batch.deleted_block_refs.max(0) as u64;
        report.deleted_artifacts += batch.deleted_artifacts.max(0) as u64;
        report.deleted_snapshots += batch.deleted_snapshots.max(0) as u64;
        report.cleared_tool_arguments += batch.cleared_tool_arguments.max(0) as u64;
        report.deleted_redaction_sessions += batch.deleted_redaction_sessions.max(0) as u64;
        if batch.expired_events <= 0 {
            break;
        }
    }

    let orphan_prompt_blocks =
        sqlx::query_file!("src/sql/usage/cleanup_orphan_usage_prompt_blocks.sql")
            .execute(&mut **conn)
            .await?;
    report.orphan_prompt_blocks_deleted = orphan_prompt_blocks.rows_affected();
    Ok(report)
}

pub(super) async fn cleanup_orphan_usage_prompt_blocks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_orphan_usage_prompt_blocks.sql")
        .execute(&mut **tx)
        .await?;
    Ok(deleted.rows_affected())
}

async fn try_acquire_usage_content_maintenance_lock(
    pool: &PgPool,
) -> Result<Option<sqlx::pool::PoolConnection<sqlx::Postgres>>> {
    let mut conn = pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "src/sql/usage/try_acquire_usage_content_maintenance_lock.sql",
        USAGE_CONTENT_MAINTENANCE_LOCK_KEY,
    )
    .fetch_one(&mut *conn)
    .await?;
    if acquired { Ok(Some(conn)) } else { Ok(None) }
}
