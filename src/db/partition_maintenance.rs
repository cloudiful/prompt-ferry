//! Settings-driven TimescaleDB chunk retention for the request family.
//!
//! Every 15 minutes the maintenance tick calls [`run_partition_maintenance`],
//! which drops whole expired chunks for the metadata parent and the six
//! normalized content parents through `drop_chunks`. Raw payloads are owned by
//! their own maintenance path because they have an independent horizon.
//!
//! Chunk drops are metadata-only catalog operations: no rows are deleted
//! row-by-row, no bloat is produced, and whole chunks disappear atomically.
//! The parent names are compile-time constants bound through `text::regclass`;
//! no chunk name or TimescaleDB internal identifier is ever constructed.
//!
//! Safety boundary: the chunk cutoff is always a whole UTC day at or before
//! yesterday's midnight, so the current UTC day chunk survives even when a
//! retention setting is misconfigured to zero. [`drop_chunks_before_today`]
//! serves the full-history admin clear, which must also remove yesterday.

use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};

pub const PARTITION_MAINTENANCE_LOCK_KEY: i64 = 0x7066_7970_6172_7438;

/// Metadata parent: request rows carry the primary reporting horizon.
const METADATA_FAMILY_PARENTS: &[&str] = &["request_records"];

/// Content parents share the normalized-content horizon. Raw payloads are
/// deliberately absent: their retention is independent and raw maintenance
/// owns their chunk drop.
const CONTENT_FAMILY_PARENTS: &[&str] = &[
    "request_record_content",
    "request_record_block_refs",
    "usage_prompt_blocks",
    "request_record_assistant_artifacts",
    "request_record_tool_calls",
    "request_record_replay_snapshots",
];

pub(crate) const RAW_PAYLOAD_PARENT: &str = "request_record_raw_payloads";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartitionMaintenanceReport {
    pub chunks_dropped: u64,
    /// Metadata instants affected by dropped metadata chunks, for the P5
    /// rollup refresh.
    pub affected_metadata: Option<AffectedRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct PartitionHorizons {
    /// Whole days the content parents survive past their chunk's day start.
    pub content_retention_days: i64,
    /// Whole days the metadata parent survives past its chunk's day start.
    pub metadata_retention_days: i64,
}

/// Metadata minutes actually affected by a prune or clear, for the P5
/// materialized-view refresh hook. `start == None` means the removal reached
/// the beginning of retained history. `end` is the last affected instant
/// (inclusive); P5 expands it to its exclusive minute ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffectedRange {
    pub start: Option<DateTime<Utc>>,
    pub end: DateTime<Utc>,
}

impl AffectedRange {
    /// A chunk drop that removed everything before `cutoff`: the range starts
    /// at the beginning of retained history and ends one microsecond before
    /// the exclusive cutoff.
    pub fn unbounded_before(cutoff: DateTime<Utc>) -> Self {
        Self {
            start: None,
            end: cutoff - ChronoDuration::microseconds(1),
        }
    }

    /// Actual deleted metadata rows, inclusive of both endpoints.
    pub fn from_rows(min: DateTime<Utc>, max: DateTime<Utc>) -> Self {
        Self {
            start: Some(min),
            end: max,
        }
    }

    /// Union of two affected ranges; an unbounded start dominates and the
    /// latest affected instant wins.
    pub fn merge(self, other: Self) -> Self {
        let start = match (self.start, other.start) {
            (Some(left), Some(right)) => Some(left.min(right)),
            _ => None,
        };
        Self {
            start,
            end: self.end.max(other.end),
        }
    }

    pub fn merge_optional(left: Option<Self>, right: Option<Self>) -> Option<Self> {
        match (left, right) {
            (Some(left), Some(right)) => Some(left.merge(right)),
            (Some(range), None) | (None, Some(range)) => Some(range),
            (None, None) => None,
        }
    }
}

/// Whole-UTC-day retention cutoff used for chunk drops.
///
/// Public maintenance clamps non-positive horizons to one day and always
/// returns a midnight at or before yesterday's, so the current UTC day chunk
/// is never selected.
pub(crate) fn chunk_retention_cutoff(now: DateTime<Utc>, retention_days: i64) -> DateTime<Utc> {
    // A horizon beyond a few centuries means "keep everything" in practice,
    // while the clamp keeps the timestamp subtraction inside chrono's range.
    let days = retention_days.clamp(1, 100_000);
    let today = now.date_naive();
    (today - ChronoDuration::days(days))
        .and_hms_opt(0, 0, 0)
        .expect("valid UTC midnight")
        .and_utc()
}

fn today_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid UTC midnight")
        .and_utc()
}

/// Tick entry point: drop expired chunks for the metadata and content parents.
/// `Ok(None)` means the advisory lock was held elsewhere.
pub async fn run_partition_maintenance(
    pool: &PgPool,
    horizons: PartitionHorizons,
) -> Result<Option<PartitionMaintenanceReport>> {
    let Some(mut conn) = try_acquire_partition_lock(pool).await? else {
        return Ok(None);
    };
    let result = run_partition_maintenance_locked(&mut conn, horizons).await;
    let result = match result {
        Ok(report) => {
            match super::usage::refresh_usage_aggregate(pool, report.affected_metadata).await {
                Ok(()) => Ok(report),
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    };
    finish_with_lock(&mut conn, result).await.map(Some)
}

/// Drop every managed parent's chunks (including raw payloads) whose whole
/// range ends before the current UTC day. Used by the full-history admin
/// clear, which must also remove yesterday. `Ok(None)` means the advisory lock
/// was held elsewhere.
pub async fn drop_chunks_before_today(pool: &PgPool) -> Result<Option<u64>> {
    let Some(mut conn) = try_acquire_partition_lock(pool).await? else {
        return Ok(None);
    };
    let result = drop_before_today_locked(&mut conn).await;
    finish_with_lock(&mut conn, result).await.map(Some)
}

async fn run_partition_maintenance_locked(
    conn: &mut PoolConnection<Postgres>,
    horizons: PartitionHorizons,
) -> Result<PartitionMaintenanceReport> {
    let now = Utc::now();
    let metadata_cutoff = chunk_retention_cutoff(now, horizons.metadata_retention_days);
    let content_cutoff = chunk_retention_cutoff(now, horizons.content_retention_days);
    let mut report = PartitionMaintenanceReport::default();
    let mut metadata_chunks = 0u64;

    for parent in METADATA_FAMILY_PARENTS {
        let dropped = drop_parent_chunks(&mut **conn, parent, metadata_cutoff).await?;
        metadata_chunks += dropped;
        report.chunks_dropped += dropped;
    }
    for parent in CONTENT_FAMILY_PARENTS {
        report.chunks_dropped += drop_parent_chunks(&mut **conn, parent, content_cutoff).await?;
    }
    report.affected_metadata =
        (metadata_chunks > 0).then(|| AffectedRange::unbounded_before(metadata_cutoff));

    Ok(report)
}

async fn drop_before_today_locked(conn: &mut PoolConnection<Postgres>) -> Result<u64> {
    let cutoff = today_midnight(Utc::now());
    let mut dropped = 0u64;
    for parent in METADATA_FAMILY_PARENTS
        .iter()
        .chain(CONTENT_FAMILY_PARENTS)
        .copied()
        .chain(std::iter::once(RAW_PAYLOAD_PARENT))
    {
        dropped += drop_parent_chunks(&mut **conn, parent, cutoff).await?;
    }
    Ok(dropped)
}

/// Drop the expired chunks of one compile-time parent and count the chunks
/// TimescaleDB actually returned.
pub(crate) async fn drop_parent_chunks(
    conn: &mut PgConnection,
    parent: &str,
    cutoff: DateTime<Utc>,
) -> Result<u64> {
    let dropped = sqlx::query_file_scalar!("src/sql/partitions/drop_chunks.sql", parent, cutoff)
        .fetch_one(conn)
        .await?;
    Ok(dropped.max(0) as u64)
}

pub(crate) async fn try_acquire_partition_lock(
    pool: &PgPool,
) -> Result<Option<PoolConnection<Postgres>>> {
    let mut conn = pool.acquire().await?;
    // A session advisory lock must never return to the pool with this
    // connection: a cancelled, failed or panicking attempt before the explicit
    // unlock would otherwise leave the maintenance key held by every future
    // user of the same pooled session.
    conn.close_on_drop();
    let acquired = sqlx::query_file_scalar!(
        "src/sql/partitions/try_acquire_lock.sql",
        PARTITION_MAINTENANCE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await?;
    if acquired { Ok(Some(conn)) } else { Ok(None) }
}

pub(crate) async fn release_partition_lock(conn: &mut PoolConnection<Postgres>) -> Result<()> {
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

/// Release the advisory lock before returning the connection to the pool.
/// If the release fails, the connection is closed instead of being reused with
/// the lock still held.
pub(crate) async fn finish_with_lock<T>(
    conn: &mut PoolConnection<Postgres>,
    result: Result<T>,
) -> Result<T> {
    match release_partition_lock(conn).await {
        Ok(()) => result,
        Err(release_error) => {
            tracing::error!(
                error = %release_error,
                "failed to release partition maintenance advisory lock; closing connection"
            );
            conn.close_on_drop();
            result.and(Err(release_error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-24T18:30:00Z")
            .expect("valid instant")
            .with_timezone(&Utc)
    }

    fn date(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("valid instant")
            .with_timezone(&Utc)
    }

    #[test]
    fn family_parents_are_disjoint_and_raw_is_separate() {
        assert!(METADATA_FAMILY_PARENTS.contains(&"request_records"));
        assert!(CONTENT_FAMILY_PARENTS.contains(&"request_record_content"));
        assert!(!CONTENT_FAMILY_PARENTS.contains(&"request_records"));
        assert!(!CONTENT_FAMILY_PARENTS.contains(&RAW_PAYLOAD_PARENT));
        let managed: Vec<&str> = METADATA_FAMILY_PARENTS
            .iter()
            .chain(CONTENT_FAMILY_PARENTS)
            .copied()
            .chain(std::iter::once(RAW_PAYLOAD_PARENT))
            .collect();
        for parent in &managed {
            assert_eq!(
                managed
                    .iter()
                    .filter(|candidate| *candidate == parent)
                    .count(),
                1,
                "{parent} must appear exactly once"
            );
        }
        assert_eq!(
            METADATA_FAMILY_PARENTS.len() + CONTENT_FAMILY_PARENTS.len() + 1,
            8,
            "the full-history clear owns all eight request-family parents"
        );
    }

    #[test]
    fn chunk_cutoffs_follow_independent_horizons() {
        let horizons = PartitionHorizons {
            content_retention_days: 2,
            metadata_retention_days: 90,
        };
        assert_eq!(
            chunk_retention_cutoff(now(), horizons.metadata_retention_days),
            date("2026-06-26T00:00:00Z")
        );
        assert_eq!(
            chunk_retention_cutoff(now(), horizons.content_retention_days),
            date("2026-09-22T00:00:00Z")
        );
    }

    #[test]
    fn chunk_cutoffs_clamp_to_whole_days_and_never_reach_today() {
        let now = now();
        let today = date("2026-09-24T00:00:00Z");
        for days in [i64::MIN, -5, 0, 1] {
            let cutoff = chunk_retention_cutoff(now, days);
            assert_eq!(cutoff, date("2026-09-23T00:00:00Z"), "days={days}");
            assert!(cutoff < today);
            assert_eq!(cutoff.timestamp() % 86_400, 0);
        }
        for days in [2, 3, 90, i64::MAX] {
            let cutoff = chunk_retention_cutoff(now, days);
            assert!(cutoff <= date("2026-09-23T00:00:00Z"), "days={days}");
            assert_eq!(cutoff.timestamp() % 86_400, 0);
        }
        assert!(chunk_retention_cutoff(now, i64::MAX) < date("1900-01-01T00:00:00Z"));
    }

    #[test]
    fn chunk_cutoff_keeps_the_current_utc_day_chunk() {
        let now = now();
        let cutoff = chunk_retention_cutoff(now, 1);
        assert_eq!(today_midnight(now) - cutoff, ChronoDuration::days(1));
    }

    #[test]
    fn affected_ranges_distinguish_no_work_from_unbounded_past() {
        assert_eq!(AffectedRange::merge_optional(None, None), None);

        let unbounded = AffectedRange::unbounded_before(date("2026-09-24T00:00:00Z"));
        assert_eq!(unbounded.start, None);
        assert_eq!(unbounded.end, date("2026-09-23T23:59:59.999999Z"));

        let rows =
            AffectedRange::from_rows(date("2026-09-24T10:00:00Z"), date("2026-09-24T10:05:00Z"));
        assert_eq!(
            AffectedRange::merge_optional(Some(unbounded), Some(rows)),
            Some(AffectedRange {
                start: None,
                end: date("2026-09-24T10:05:00Z"),
            })
        );
    }

    #[test]
    fn affected_row_ranges_union_with_exclusive_edges() {
        let left =
            AffectedRange::from_rows(date("2026-09-24T10:00:00Z"), date("2026-09-24T10:05:00Z"));
        let right =
            AffectedRange::from_rows(date("2026-09-24T09:58:00Z"), date("2026-09-24T10:10:00Z"));
        assert_eq!(
            left.merge(right),
            AffectedRange {
                start: Some(date("2026-09-24T09:58:00Z")),
                end: date("2026-09-24T10:10:00Z"),
            }
        );
        let touching =
            AffectedRange::from_rows(date("2026-09-24T10:10:00Z"), date("2026-09-24T10:10:00Z"));
        assert_eq!(left.merge(touching).end, date("2026-09-24T10:10:00Z"));
    }
}
