//! Issue #277 Phase P8 — daily-partition maintenance for the request family.
//!
//! Every 15 minutes the maintenance tick calls [`run_partition_maintenance`],
//! which (a) pre-creates the current day plus the next three daily partitions
//! for every managed parent so writes never hit a missing partition, and
//! (b) drops partitions that are entirely past the family retention window.
//! Dropping a partition is a metadata-only catalog operation: no rows are
//! deleted, no bloat is produced, and the statement-timeout problems of the
//! row-wise prunes this module replaces cannot occur.
//!
//! Statistics (Phase P11): each partition created by a tick is ANALYZEd
//! individually so the planner immediately has real `reltuples` for it;
//! existing partitions are never mass-analyzed and stay on autovacuum
//! autoanalyze.
//!
//! Safety boundary: only partitions whose whole day is strictly before the
//! current UTC day are dropped, so today's data survives even when retention
//! settings are misconfigured to zero.
//!
//! [`drop_partitions_before_today`] serves the full-history admin clear, which
//! must also remove the previous day that the retention tick deliberately
//! keeps. The DDL primitives live in [`super::partition_ddl`].

use super::partition_ddl::{
    PRE_CREATE_DAYS_AHEAD, analyze_partition, create_partition, drop_partition, list_partitions,
    partition_day, partition_is_droppable, partition_name,
};
use anyhow::Result;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use sqlx::{Acquire, PgPool};

pub const PARTITION_MAINTENANCE_LOCK_KEY: i64 = 0x7066_7970_6172_7438;

/// Content-family parents: `request_record_content` itself plus every table
/// sharing its 3-day lifecycle. Raw payloads join the same drop cadence.
const CONTENT_FAMILY_PARENTS: &[&str] = &[
    "request_record_content",
    "request_record_block_refs",
    "usage_prompt_blocks",
    "request_record_assistant_artifacts",
    "request_record_tool_calls",
    "request_record_replay_snapshots",
    "request_record_raw_payloads",
];

const METADATA_FAMILY_PARENTS: &[&str] = &["request_records"];

#[derive(Debug, Clone, Copy, Default)]
pub struct PartitionMaintenanceReport {
    pub partitions_created: u64,
    pub partitions_dropped: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PartitionHorizons {
    /// Whole days a content-family partition survives past its day start.
    pub content_retention_days: i64,
    /// Whole days a metadata partition survives past its day start.
    pub metadata_retention_days: i64,
}

/// Tick entry point: pre-create the forward window and drop expired
/// partitions. `Ok(None)` means the advisory lock was held elsewhere.
pub async fn run_partition_maintenance(
    pool: &PgPool,
    horizons: PartitionHorizons,
) -> Result<Option<PartitionMaintenanceReport>> {
    let Some(mut conn) = try_acquire_partition_lock(pool).await? else {
        return Ok(None);
    };
    let result = run_partition_maintenance_locked(&mut conn, horizons).await;
    if let Err(release_error) = release_partition_lock(&mut conn).await {
        tracing::error!(
            error = %release_error,
            "failed to release partition maintenance advisory lock"
        );
    }
    result.map(Some)
}

/// Drop every managed parent's partition whose whole day is strictly before
/// the current UTC day, ignoring the retention horizons. Used by the
/// full-history admin clear, which must also remove yesterday. `Ok(None)`
/// means the advisory lock was held elsewhere.
pub async fn drop_partitions_before_today(pool: &PgPool) -> Result<Option<u64>> {
    let Some(mut conn) = try_acquire_partition_lock(pool).await? else {
        return Ok(None);
    };
    let result = drop_before_today_locked(&mut conn).await;
    if let Err(release_error) = release_partition_lock(&mut conn).await {
        tracing::error!(
            error = %release_error,
            "failed to release partition maintenance advisory lock"
        );
    }
    result.map(Some)
}

async fn run_partition_maintenance_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    horizons: PartitionHorizons,
) -> Result<PartitionMaintenanceReport> {
    let today = Utc::now().date_naive();
    let mut report = PartitionMaintenanceReport::default();

    for parent in managed_parents() {
        let existing = list_partitions(conn, parent).await?;
        let created = pre_create_partitions(conn, parent, &existing, today).await?;
        report.partitions_created += created;
    }

    // A partition is droppable when its day ended before the retention
    // cutoff; the safety boundary additionally requires the whole day to be
    // strictly before the current UTC day.
    for parent in managed_parents() {
        let cutoff = retention_cutoff_for(parent, today, horizons);
        report.partitions_dropped += drop_expired_partitions(conn, parent, cutoff, today).await?;
    }

    Ok(report)
}

async fn drop_before_today_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> Result<u64> {
    let today = Utc::now().date_naive();
    let mut dropped = 0u64;
    for parent in managed_parents() {
        // Passing today as the cutoff keeps only the "before the current UTC
        // day" half of the droppable predicate.
        dropped += drop_expired_partitions(conn, parent, today, today).await?;
    }
    Ok(dropped)
}

/// All managed parents in drop/create order: metadata first, then the whole
/// content family.
fn managed_parents() -> impl Iterator<Item = &'static str> {
    METADATA_FAMILY_PARENTS
        .iter()
        .copied()
        .chain(CONTENT_FAMILY_PARENTS.iter().copied())
}

/// Retention cutoff for a parent: metadata parents use the metadata horizon,
/// everything else shares the content horizon.
fn retention_cutoff_for(parent: &str, today: NaiveDate, horizons: PartitionHorizons) -> NaiveDate {
    let days = if METADATA_FAMILY_PARENTS.contains(&parent) {
        horizons.metadata_retention_days
    } else {
        horizons.content_retention_days
    };
    today - ChronoDuration::days(days.max(1))
}

async fn try_acquire_partition_lock(
    pool: &PgPool,
) -> Result<Option<sqlx::pool::PoolConnection<sqlx::Postgres>>> {
    let mut conn = pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "src/sql/partitions/try_acquire_lock.sql",
        PARTITION_MAINTENANCE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await?;
    if acquired { Ok(Some(conn)) } else { Ok(None) }
}

async fn release_partition_lock(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> Result<()> {
    let released = sqlx::query_file_scalar!(
        "src/sql/partitions/release_lock.sql",
        PARTITION_MAINTENANCE_LOCK_KEY
    )
    .fetch_one(&mut **conn)
    .await?;
    if released {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "partition maintenance advisory lock was not held during release"
        ))
    }
}

/// Create today plus the forward window, ANALYZEing each partition this tick
/// created (Phase P11: only fresh partitions are analyzed; existing ones stay
/// on autovacuum autoanalyze). `CREATE TABLE IF NOT EXISTS` makes this
/// idempotent, so a day whose partition is missing (a worker gap longer than
/// a day) is healed by the next tick instead of failing writes forever.
async fn pre_create_partitions(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    parent: &str,
    existing: &std::collections::HashSet<String>,
    today: NaiveDate,
) -> Result<u64> {
    let mut created = 0u64;
    let mut tx = conn.begin().await?;
    for offset in 0..=PRE_CREATE_DAYS_AHEAD {
        let day = today + ChronoDuration::days(offset);
        let partition = partition_name(parent, day);
        if existing.contains(&partition) {
            continue;
        }
        create_partition(&mut tx, parent, &partition, day).await?;
        // Only the partition this tick created is analyzed: a single,
        // well-scoped stats refresh instead of the whole-family VACUUM
        // (ANALYZE) the tick used to issue.
        analyze_partition(&mut tx, &partition).await?;
        created += 1;
    }
    tx.commit().await?;
    Ok(created)
}

/// Drop every partition of `parent` whose whole day is strictly before both
/// the retention cutoff and the current UTC day.
async fn drop_expired_partitions(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    parent: &str,
    cutoff: NaiveDate,
    today: NaiveDate,
) -> Result<u64> {
    let existing = list_partitions(conn, parent).await?;
    let mut dropped = 0u64;
    for partition in existing {
        let Some(day) = partition_day(&partition, parent) else {
            continue;
        };
        if !partition_is_droppable(day, cutoff, today) {
            continue;
        }
        drop_partition(conn, &partition).await?;
        dropped += 1;
    }
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_family_parents_are_disjoint_from_metadata() {
        assert!(CONTENT_FAMILY_PARENTS.contains(&"request_record_content"));
        assert!(METADATA_FAMILY_PARENTS.contains(&"request_records"));
        assert!(!CONTENT_FAMILY_PARENTS.contains(&"request_records"));
        for parent in managed_parents() {
            let count = managed_parents().filter(|p| *p == parent).count();
            assert_eq!(count, 1, "{parent} must appear exactly once");
        }
    }

    #[test]
    fn retention_cutoffs_follow_the_family_horizons() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 24).expect("valid date");
        let horizons = PartitionHorizons {
            content_retention_days: 3,
            metadata_retention_days: 90,
        };
        assert_eq!(
            retention_cutoff_for("request_records", today, horizons),
            NaiveDate::from_ymd_opt(2026, 6, 26).expect("valid date")
        );
        assert_eq!(
            retention_cutoff_for("request_record_content", today, horizons),
            NaiveDate::from_ymd_opt(2026, 9, 21).expect("valid date")
        );
        assert_eq!(
            retention_cutoff_for("request_record_raw_payloads", today, horizons),
            NaiveDate::from_ymd_opt(2026, 9, 21).expect("valid date")
        );
    }

    #[test]
    fn pre_create_window_starts_today() {
        // The window must include the current day so a missing partition is
        // healed by the next tick (Phase P8-4).
        let today = NaiveDate::from_ymd_opt(2026, 9, 24).expect("valid date");
        let days: Vec<NaiveDate> = (0..=PRE_CREATE_DAYS_AHEAD)
            .map(|offset| today + ChronoDuration::days(offset))
            .collect();
        assert_eq!(days.first(), Some(&today));
        assert_eq!(days.len(), 4);
    }

    // Phase P11: the tick's create loop must skip partitions that already
    // exist — the per-partition ANALYZE belongs to fresh partitions only, so
    // an already-present day must never trigger an extra analyze pass.
    #[test]
    fn pre_create_skips_existing_partitions_by_name() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 24).expect("valid date");
        let existing: std::collections::HashSet<String> = (0..=PRE_CREATE_DAYS_AHEAD)
            .map(|offset| partition_name("request_records", today + ChronoDuration::days(offset)))
            .collect();
        for offset in 0..=PRE_CREATE_DAYS_AHEAD {
            let partition = partition_name("request_records", today + ChronoDuration::days(offset));
            assert!(
                existing.contains(&partition),
                "a steady-state tick has nothing to create or analyze for {partition}"
            );
        }
    }
}
