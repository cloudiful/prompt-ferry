use super::content_maintenance::cleanup_orphan_usage_prompt_blocks;
use super::{RequestRecordClearQuery, UsageClearScope};
use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use sqlx::{Acquire, PgPool};

const METADATA_PRUNE_BATCH_SIZE: i64 = 500;
const METADATA_PRUNE_LOCK_KEY: i64 = 0x7072_756e_654d_6574;

#[derive(Debug)]
struct RequestRecordPruneBatch {
    deleted_count: i64,
    orphan_leases_deleted: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordPruneReport {
    pub deleted: u64,
    pub protected_by_billing: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordClearReport {
    pub deleted: u64,
    pub deleted_prompt_blocks: u64,
    pub protected_by_billing: u64,
}

pub async fn prune_usage_events(
    pool: &PgPool,
    retention_days: i64,
) -> Result<RequestRecordPruneReport> {
    let mut conn = acquire_metadata_prune_lock(pool).await?;
    let result = run_metadata_prune_locked(&mut conn, retention_days).await;
    let result = match result {
        Ok(report) => {
            let mut tx = conn.begin().await?;
            cleanup_orphan_usage_prompt_blocks(&mut tx).await?;
            tx.commit().await?;
            Ok(report)
        }
        Err(error) => Err(error),
    };
    release_metadata_prune_lock(&mut conn, result).await
}

pub async fn run_usage_metadata_maintenance(
    pool: &PgPool,
    retention_days: i64,
) -> Result<Option<RequestRecordPruneReport>> {
    let Some(mut conn) = try_acquire_metadata_prune_lock(pool).await? else {
        return Ok(None);
    };
    let result = run_metadata_prune_locked(&mut conn, retention_days).await;
    release_metadata_prune_lock(&mut conn, result)
        .await
        .map(Some)
}

async fn run_metadata_prune_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    retention_days: i64,
) -> Result<RequestRecordPruneReport> {
    let cutoff = Utc::now() - ChronoDuration::days(retention_days.max(1));
    let protected_by_billing = sqlx::query_file_scalar!(
        "src/sql/usage/count_billing_protected_request_records.sql",
        cutoff,
    )
    .fetch_one(&mut **conn)
    .await?;
    let mut report = RequestRecordPruneReport {
        deleted: 0,
        protected_by_billing: protected_by_billing.max(0) as u64,
    };

    loop {
        let mut tx = conn.begin().await?;
        let batch = sqlx::query_file_as!(
            RequestRecordPruneBatch,
            "src/sql/usage/prune_usage_events.sql",
            cutoff,
            METADATA_PRUNE_BATCH_SIZE,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        report.deleted += batch.deleted_count.max(0) as u64;
        if batch.orphan_leases_deleted > 0 {
            tracing::debug!(
                deleted = batch.orphan_leases_deleted,
                "deleted orphan request record leases during metadata maintenance"
            );
        }
        if batch.deleted_count <= 0 {
            break;
        }
    }

    Ok(report)
}

async fn acquire_metadata_prune_lock(
    pool: &PgPool,
) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>> {
    let mut conn = pool.acquire().await?;
    sqlx::query_file!(
        "src/sql/usage/acquire_metadata_prune_lock.sql",
        METADATA_PRUNE_LOCK_KEY,
    )
    .execute(&mut *conn)
    .await?;
    Ok(conn)
}

async fn try_acquire_metadata_prune_lock(
    pool: &PgPool,
) -> Result<Option<sqlx::pool::PoolConnection<sqlx::Postgres>>> {
    let mut conn = pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "src/sql/usage/try_acquire_metadata_prune_lock.sql",
        METADATA_PRUNE_LOCK_KEY,
    )
    .fetch_one(&mut *conn)
    .await?;
    if acquired { Ok(Some(conn)) } else { Ok(None) }
}

async fn release_metadata_prune_lock(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    result: Result<RequestRecordPruneReport>,
) -> Result<RequestRecordPruneReport> {
    let released = sqlx::query_file_scalar!(
        "src/sql/usage/release_metadata_prune_lock.sql",
        METADATA_PRUNE_LOCK_KEY,
    )
    .fetch_one(&mut **conn)
    .await;
    match (result, released) {
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(unlock_error)) => {
            tracing::error!(error = %unlock_error, "failed to release metadata prune advisory lock");
            Err(error)
        }
        (Ok(report), Ok(true)) => Ok(report),
        (Ok(_), Ok(false)) => Err(anyhow::anyhow!(
            "metadata prune advisory lock was not held during release"
        )),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

pub async fn clear_usage_events(
    pool: &PgPool,
    query: RequestRecordClearQuery,
) -> Result<RequestRecordClearReport> {
    let mut tx = pool.begin().await?;
    let result = match query.scope {
        UsageClearScope::CurrentUser => {
            sqlx::query_file_as!(
                RequestRecordClearBatch,
                "src/sql/usage/clear_usage_events_current_user.sql",
                query.visible_user_id,
                query.start_at,
                query.end_at,
            )
            .fetch_one(&mut *tx)
            .await?
        }
        UsageClearScope::AllUsers => {
            sqlx::query_file_as!(
                RequestRecordClearBatch,
                "src/sql/usage/clear_usage_events_all_users.sql",
                query.start_at,
                query.end_at,
            )
            .fetch_one(&mut *tx)
            .await?
        }
        UsageClearScope::TargetUser => {
            sqlx::query_file_as!(
                RequestRecordClearBatch,
                "src/sql/usage/clear_usage_events_target_user.sql",
                query.target_user_id,
                query.start_at,
                query.end_at,
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };
    sqlx::query_file!("src/sql/usage/cleanup_orphan_request_record_leases.sql")
        .execute(&mut *tx)
        .await?;
    let deleted_prompt_blocks = cleanup_orphan_usage_prompt_blocks(&mut tx).await?;
    tx.commit().await?;
    Ok(RequestRecordClearReport {
        deleted: result.deleted_count.max(0) as u64,
        deleted_prompt_blocks,
        protected_by_billing: result.protected_by_billing.max(0) as u64,
    })
}

#[derive(Debug)]
struct RequestRecordClearBatch {
    deleted_count: i64,
    protected_by_billing: i64,
}
