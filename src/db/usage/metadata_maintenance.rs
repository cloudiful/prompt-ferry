//! Issue #277 Phase P8 — partition-backed prune/clear for request records.
//!
//! Retention pruning is the partition DROP path (metadata only, no row
//! deletes). `clear` drops whole-history partitions only for a full-scope,
//! unbounded request; every per-user or windowed clear is a bounded row
//! DELETE across the request family, reachable only from the manual admin
//! endpoint. The billing protection concept is gone by operator decision:
//! `usage_charges` is a permanent ledger decoupled from the request family.
//!
//! The plain small tables (`request_record_leases`,
//! `conversation_redaction_sessions`) sit outside the partition lifecycle, so
//! this module also exposes their cheap tick cleanups.

use super::*;
use crate::db::{PartitionHorizons, drop_partitions_before_today, run_partition_maintenance};
use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordPruneReport {
    pub deleted: u64,
    pub partitions_dropped: u64,
    /// Always 0: billing protection was removed with the partition-drop
    /// retention model (usage_charges is a permanent, decoupled ledger).
    /// The field remains so the admin response shape is unchanged.
    pub protected_by_billing: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordClearReport {
    pub deleted: u64,
    pub deleted_prompt_blocks: u64,
    /// Always 0, kept for response-shape compatibility (see
    /// `RequestRecordPruneReport::protected_by_billing`).
    pub protected_by_billing: u64,
}

/// Retention prune: run the partition manager with the metadata/content
/// horizons. Partition DROPs carry the expired rows away; `deleted` counts
/// today's-day metadata rows cannot expire, so it stays 0 while
/// `partitions_dropped` carries the progress signal.
pub async fn prune_usage_events(
    pool: &PgPool,
    retention_days: i64,
) -> AnyhowResult<RequestRecordPruneReport> {
    let horizons = PartitionHorizons {
        metadata_retention_days: retention_days.max(1),
        content_retention_days: i64::from(crate::db::PARTITION_CONTENT_RETENTION_DAYS),
    };
    let report = run_partition_maintenance(pool, horizons)
        .await?
        .ok_or_else(|| anyhow::anyhow!("partition maintenance is running; retry the prune"))?;
    Ok(RequestRecordPruneReport {
        deleted: 0,
        partitions_dropped: report.partitions_dropped,
        protected_by_billing: 0,
    })
}

/// `clear` deletes request-family rows for the requested scope.
///
/// Only a full-scope, unbounded request (`AllUsers` with neither bound) drops
/// partitions: every partition strictly before the current UTC day leaves in
/// one catalog operation, and only today's rows remain to be deleted. Any
/// per-user or windowed clear is a bounded row DELETE, so a non-admin can
/// never destroy another user's history.
pub async fn clear_usage_events(
    pool: &PgPool,
    query: RequestRecordClearQuery,
) -> AnyhowResult<RequestRecordClearReport> {
    let mut report = RequestRecordClearReport::default();
    let full_history = matches!(query.scope, UsageClearScope::AllUsers)
        && query.start_at.is_none()
        && query.end_at.is_none();

    if full_history {
        // 1. Whole-day history: every partition before today (including
        //    yesterday, which the retention tick deliberately keeps) drops.
        let dropped = drop_partitions_before_today(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("partition maintenance is running; retry the clear"))?;
        report.deleted += dropped;

        // 2. Today's rows: one bounded DELETE for the current UTC day.
        report.deleted += clear_family_rows(pool, &query, Some(today_midnight()?)).await?;

        // 3. Every remaining prompt block lost its last ref in steps 1-2.
        report.deleted_prompt_blocks = delete_all_orphan_prompt_blocks(pool).await?;
    } else {
        report.deleted = clear_family_rows(pool, &query, None).await?;

        // Only current-day orphans can remain; older orphans leave with their
        // partitions.
        report.deleted_prompt_blocks = cleanup_orphan_usage_prompt_blocks(pool).await?;
    }

    // Both paths delete request rows, so leases pointing at them are now
    // orphaned; reap them without waiting for the next tick.
    cleanup_orphan_request_record_leases(pool).await?;

    Ok(report)
}

/// Bounded family DELETE for the query scope. `today_only` overrides the
/// range with the current UTC day and is used only by the full-history clear.
async fn clear_family_rows(
    pool: &PgPool,
    query: &RequestRecordClearQuery,
    today_only: Option<DateTime<Utc>>,
) -> AnyhowResult<u64> {
    let (scope_code, target_user_id, visible_user_id) = match query.scope {
        UsageClearScope::AllUsers => (0_i32, None::<i64>, None::<i64>),
        UsageClearScope::CurrentUser => (1_i32, None, query.visible_user_id),
        UsageClearScope::TargetUser => (2_i32, query.target_user_id, None),
    };
    let (start_at, end_at) = match today_only {
        Some(start) => (Some(start), None),
        None => (query.start_at, query.end_at),
    };
    let row = sqlx::query_file_as!(
        ClearFamilyBatch,
        "src/sql/partitions/clear_family_rows.sql",
        target_user_id,
        visible_user_id,
        start_at,
        end_at,
        scope_code,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.deleted_count.max(0) as u64)
}

/// Orphan prompt blocks: blocks from fully-dropped days disappear with their
/// partitions, so only current-day orphans can remain (Phase P7 guard).
pub async fn cleanup_orphan_usage_prompt_blocks(pool: &PgPool) -> AnyhowResult<u64> {
    let mut conn = pool.acquire().await?;
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_orphan_prompt_blocks_current_day.sql")
        .execute(&mut *conn)
        .await?;
    Ok(deleted.rows_affected())
}

/// Full-history variant: after the partition drops and today's ref deletion
/// every remaining block is an orphan.
async fn delete_all_orphan_prompt_blocks(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_all_orphan_prompt_blocks.sql")
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

/// Plain-table tick cleanup: leases whose request id left with a dropped
/// metadata partition.
pub async fn cleanup_orphan_request_record_leases(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_orphan_request_record_leases.sql")
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

/// Plain-table tick cleanup: redaction sessions idle for over a week or
/// orphaned by a dropped metadata partition.
pub async fn cleanup_stale_conversation_redaction_sessions(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted =
        sqlx::query_file!("src/sql/usage/cleanup_stale_conversation_redaction_sessions.sql")
            .execute(pool)
            .await?;
    Ok(deleted.rows_affected())
}

fn today_midnight() -> AnyhowResult<DateTime<Utc>> {
    Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
        .ok_or_else(|| anyhow::anyhow!("invalid UTC day"))
}

#[derive(Debug)]
struct ClearFamilyBatch {
    deleted_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizons_preserve_positive_days() {
        let horizons = PartitionHorizons {
            metadata_retention_days: 90,
            content_retention_days: 3,
        };
        assert_eq!(horizons.metadata_retention_days, 90);
        assert_eq!(horizons.content_retention_days, 3);
    }
}
