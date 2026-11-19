//! Issue #69 Phase P4b — chunk-backed prune/clear for request records.
//!
//! Retention pruning drops whole expired TimescaleDB chunks (metadata only, no
//! row deletes). `clear` drops whole-history chunks only for a full-scope,
//! unbounded request; every per-user or windowed clear is a bounded row
//! DELETE across the request family, reachable only from the manual admin
//! endpoint. The billing protection concept is gone by operator decision:
//! `usage_charges` is a permanent ledger decoupled from the request family.
//!
//! The plain small tables (`request_record_leases`,
//! `conversation_redaction_sessions`) sit outside the chunk lifecycle, so this
//! module also exposes their cheap tick cleanups.

use super::*;
use crate::db::{
    AffectedRange, PARTITION_MAINTENANCE_LOCK_KEY, PartitionHorizons, drop_chunks_before_today,
    run_partition_maintenance,
};
use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordPruneReport {
    pub chunks_dropped: u64,
    /// Internal P5 hook: metadata minutes affected by chunk drops.
    pub affected_metadata: Option<AffectedRange>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestRecordClearReport {
    /// Bounded SQL row deletes only; rows carried away by dropped chunks are
    /// counted in `chunks_dropped`, not here.
    pub deleted: u64,
    pub deleted_prompt_blocks: u64,
    pub chunks_dropped: u64,
    /// Internal P5 hook: metadata minutes affected by chunk drops and row
    /// deletes, or `None` when no metadata changed.
    pub affected_metadata: Option<AffectedRange>,
}

/// Retention prune: run chunk maintenance with the current metadata/content
/// horizons. Chunk DROPs carry the expired rows away, so `chunks_dropped` is
/// the progress signal and no raw row scan is needed.
pub async fn prune_usage_events(
    pool: &PgPool,
    metadata_retention_days: i64,
    content_retention_days: i64,
) -> AnyhowResult<RequestRecordPruneReport> {
    let horizons = PartitionHorizons {
        metadata_retention_days: metadata_retention_days.max(1),
        content_retention_days: content_retention_days.max(1),
    };
    let report = run_partition_maintenance(pool, horizons)
        .await?
        .ok_or_else(|| anyhow::anyhow!("partition maintenance is running; retry the prune"))?;
    Ok(RequestRecordPruneReport {
        chunks_dropped: report.chunks_dropped,
        affected_metadata: report.affected_metadata,
    })
}

/// `clear` deletes request-family rows for the requested scope.
///
/// Only a full-scope, unbounded request (`AllUsers` with neither bound) drops
/// chunks: every chunk strictly before the current UTC day leaves in catalog
/// operations, and only today's rows remain to be deleted. Any per-user or
/// windowed clear is a bounded row DELETE, so a non-admin can never destroy
/// another user's history.
///
/// The bounded row delete is one READ COMMITTED transaction: it holds the
/// partition advisory key, captures the candidate envelope and block hashes,
/// locks and decompresses every touched chunk, then deletes the request family
/// and only the candidate-related old prompt blocks before commit. A crash or
/// error rolls the whole transaction back, including the decompression. The
/// full-history path still runs its catalog chunk drop before the bounded
/// current-day transaction; the two are not one transaction, which is the
/// existing documented behavior.
pub async fn clear_usage_events(
    pool: &PgPool,
    query: RequestRecordClearQuery,
) -> AnyhowResult<RequestRecordClearReport> {
    let mut report = RequestRecordClearReport::default();
    let full_history = matches!(query.scope, UsageClearScope::AllUsers)
        && query.start_at.is_none()
        && query.end_at.is_none();

    if full_history {
        let today_start = today_midnight()?;

        // 1. Whole-day history: every chunk before today (including
        //    yesterday, which the retention tick deliberately keeps) drops.
        let dropped = drop_chunks_before_today(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("partition maintenance is running; retry the clear"))?;
        report.chunks_dropped = dropped;

        // 2. Today's rows: one bounded DELETE for the current UTC day.
        let batch = clear_family_rows(pool, &query, Some(today_start)).await?;
        report.deleted = batch.deleted_count.max(0) as u64;
        let chunk_range = (dropped > 0).then(|| AffectedRange::unbounded_before(today_start));
        report.affected_metadata =
            AffectedRange::merge_optional(chunk_range, batch.affected_range());

        // 3. Every remaining prompt block lost its last ref in steps 1-2.
        report.deleted_prompt_blocks =
            batch.deleted_prompt_blocks + delete_all_orphan_prompt_blocks(pool).await?;
        // 4. Only sessions whose request row is gone; a live but old session
        //    is untouched (unlike the tick's seven-day cleanup).
        cleanup_orphan_conversation_redaction_sessions(pool).await?;
    } else {
        let batch = clear_family_rows(pool, &query, None).await?;
        report.deleted = batch.deleted_count.max(0) as u64;
        report.affected_metadata = batch.affected_range();
        report.deleted_prompt_blocks = batch.deleted_prompt_blocks;
    }

    // Both paths delete request rows, so leases pointing at them are now
    // orphaned; reap them without waiting for the next tick.
    cleanup_orphan_request_record_leases(pool).await?;
    super::aggregate::refresh_usage_aggregate(pool, report.affected_metadata).await?;

    Ok(report)
}

/// Atomic bounded family DELETE for the query scope. `today_only` overrides
/// the range with the current UTC day and is used only by the full-history
/// clear.
///
/// Every chunk the DELETE can touch is locked in ROW SHARE mode; after all
/// locks are held, each chunk's compression state is re-read in the same
/// READ COMMITTED transaction, so a compression that committed while the clear
/// waited for the lock is decompressed even though the chunk list saw the old
/// rowstore state. Timescale's per-statement decompression guard therefore
/// never sees the cold rows, and the policy cannot compress a hot chunk while
/// the transaction is open. The advisory key is transaction-scoped: network or
/// statement failure rolls back both the decompression and every delete.
async fn clear_family_rows(
    pool: &PgPool,
    query: &RequestRecordClearQuery,
    today_only: Option<DateTime<Utc>>,
) -> AnyhowResult<ClearFamilyBatch> {
    let (scope_code, target_user_id, visible_user_id) = match query.scope {
        UsageClearScope::AllUsers => (0_i32, None::<i64>, None::<i64>),
        UsageClearScope::CurrentUser => (1_i32, None, query.visible_user_id),
        UsageClearScope::TargetUser => (2_i32, query.target_user_id, None),
    };
    let (start_at, end_at) = match today_only {
        Some(start) => (Some(start), None),
        None => (query.start_at, query.end_at),
    };

    let mut tx = pool.begin().await?;
    sqlx::query_file!("src/sql/partitions/clear_transaction_mode.sql")
        .execute(&mut *tx)
        .await?;
    let acquired = sqlx::query_file_scalar!(
        "src/sql/partitions/try_clear_lock.sql",
        PARTITION_MAINTENANCE_LOCK_KEY
    )
    .fetch_one(&mut *tx)
    .await?;
    if !acquired {
        return Err(anyhow::anyhow!(
            "partition maintenance is running; retry the clear"
        ));
    }

    let envelope = sqlx::query_file!(
        "src/sql/partitions/clear_candidate_envelope.sql",
        target_user_id,
        visible_user_id,
        start_at,
        end_at,
        scope_code,
    )
    .fetch_one(&mut *tx)
    .await?;
    let (Some(min_created_at), Some(max_created_at)) =
        (envelope.min_created_at, envelope.max_created_at)
    else {
        tx.commit().await?;
        return Ok(ClearFamilyBatch {
            deleted_count: 0,
            min_created_at: None,
            max_created_at: None,
            deleted_prompt_blocks: 0,
        });
    };

    let block_hashes = sqlx::query_file_scalar!(
        "src/sql/partitions/clear_candidate_block_hashes.sql",
        target_user_id,
        visible_user_id,
        start_at,
        end_at,
        scope_code,
    )
    .fetch_all(&mut *tx)
    .await?;

    let chunks = sqlx::query_file!(
        "src/sql/partitions/clear_chunks_to_decompress.sql",
        min_created_at,
        max_created_at,
        block_hashes.as_slice(),
    )
    .fetch_all(&mut *tx)
    .await?;
    for chunk in &chunks {
        sqlx::query(sqlx::AssertSqlSafe(chunk.lock_sql.as_str()))
            .execute(&mut *tx)
            .await?;
    }
    for chunk in &chunks {
        let is_compressed = sqlx::query_file_scalar!(
            "src/sql/partitions/clear_chunk_compression_state.sql",
            chunk.chunk_schema,
            chunk.chunk_name,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chunk {}.{} compression state is missing; retry the clear",
                chunk.chunk_schema,
                chunk.chunk_name
            )
        })?;
        if is_compressed {
            sqlx::query_file!(
                "src/sql/partitions/decompress_clear_chunk.sql",
                chunk.chunk_schema,
                chunk.chunk_name,
            )
            .fetch_one(&mut *tx)
            .await?;
        }
    }

    let row = sqlx::query_file!(
        "src/sql/partitions/clear_family_rows.sql",
        target_user_id,
        visible_user_id,
        start_at,
        end_at,
        scope_code,
        min_created_at,
        max_created_at,
    )
    .fetch_one(&mut *tx)
    .await?;
    let mut batch = ClearFamilyBatch {
        deleted_count: row.deleted_count,
        min_created_at: row.min_created_at,
        max_created_at: row.max_created_at,
        deleted_prompt_blocks: 0,
    };
    batch.deleted_prompt_blocks = sqlx::query_file!(
        "src/sql/usage/cleanup_orphan_prompt_blocks_for_clear.sql",
        block_hashes.as_slice(),
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(batch)
}

/// Full-history variant: after the chunk drops and today's ref deletion every
/// remaining block is an orphan.
async fn delete_all_orphan_prompt_blocks(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_all_orphan_prompt_blocks.sql")
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

/// Plain-table clear cleanup: redaction sessions whose request row is gone.
pub async fn cleanup_orphan_conversation_redaction_sessions(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted =
        sqlx::query_file!("src/sql/usage/cleanup_orphan_conversation_redaction_sessions.sql")
            .execute(pool)
            .await?;
    Ok(deleted.rows_affected())
}

/// Plain-table tick cleanup: leases whose request id left with a dropped
/// metadata chunk.
pub async fn cleanup_orphan_request_record_leases(pool: &PgPool) -> AnyhowResult<u64> {
    let deleted = sqlx::query_file!("src/sql/usage/cleanup_orphan_request_record_leases.sql")
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

/// Plain-table tick cleanup: redaction sessions idle for over a week or
/// orphaned by a dropped metadata chunk.
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
    min_created_at: Option<DateTime<Utc>>,
    max_created_at: Option<DateTime<Utc>>,
    deleted_prompt_blocks: u64,
}

impl ClearFamilyBatch {
    /// Metadata minutes actually deleted by the bounded family DELETE.
    fn affected_range(&self) -> Option<AffectedRange> {
        match (self.min_created_at, self.max_created_at) {
            (Some(min), Some(max)) => Some(AffectedRange::from_rows(min, max)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("valid instant")
            .with_timezone(&Utc)
    }

    #[test]
    fn horizons_preserve_positive_days() {
        let horizons = PartitionHorizons {
            metadata_retention_days: 90,
            content_retention_days: 3,
        };
        assert_eq!(horizons.metadata_retention_days, 90);
        assert_eq!(horizons.content_retention_days, 3);
    }

    #[test]
    fn clear_batch_reports_no_range_without_deleted_rows() {
        let batch = ClearFamilyBatch {
            deleted_count: 0,
            min_created_at: None,
            max_created_at: None,
            deleted_prompt_blocks: 0,
        };
        assert_eq!(batch.affected_range(), None);
    }

    #[test]
    fn clear_batch_range_covers_actual_deleted_rows() {
        let batch = ClearFamilyBatch {
            deleted_count: 2,
            min_created_at: Some(instant("2026-09-24T10:00:00Z")),
            max_created_at: Some(instant("2026-09-24T10:05:30Z")),
            deleted_prompt_blocks: 0,
        };
        assert_eq!(
            batch.affected_range(),
            Some(AffectedRange::from_rows(
                instant("2026-09-24T10:00:00Z"),
                instant("2026-09-24T10:05:30Z"),
            ))
        );
    }

    #[test]
    fn full_history_range_unions_chunk_drop_with_hot_rows() {
        let today = Utc
            .with_ymd_and_hms(2026, 9, 24, 0, 0, 0)
            .single()
            .expect("valid instant");
        let chunk = AffectedRange::unbounded_before(today);
        let hot = AffectedRange::from_rows(
            instant("2026-09-24T09:00:00Z"),
            instant("2026-09-24T09:59:00Z"),
        );
        assert_eq!(
            AffectedRange::merge_optional(Some(chunk), Some(hot)),
            Some(AffectedRange {
                start: None,
                end: instant("2026-09-24T09:59:00Z"),
            })
        );
    }
}
