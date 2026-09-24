//! Issue #277 Phase P8 — DDL primitives for the daily-partition manager.
//!
//! `CREATE`/`DROP` statements are assembled from compile-time parent
//! constants, the controlled `<parent>_<YYYYMMDD>` name builder, and
//! date-formatted bounds, then executed through `sqlx::AssertSqlSafe` because
//! PostgreSQL cannot bind identifiers.

use anyhow::Result;
use chrono::{Duration as ChronoDuration, NaiveDate};
use sqlx::postgres::PgConnection;
use std::collections::HashSet;

/// Days to keep available ahead of the current UTC day.
pub(crate) const PRE_CREATE_DAYS_AHEAD: i64 = 3;

#[derive(Debug, sqlx::FromRow)]
struct PartitionRow {
    name: String,
}

/// Direct children of a partition parent, resolved through the connection
/// search_path so tests operate inside their own schema.
pub(crate) async fn list_partitions(
    conn: &mut PgConnection,
    parent: &str,
) -> Result<HashSet<String>> {
    let rows = sqlx::query_file_as!(PartitionRow, "src/sql/partitions/list.sql", parent)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(|row| row.name).collect())
}

/// `CREATE TABLE IF NOT EXISTS` for one whole UTC day of a managed parent.
pub(crate) async fn create_partition(
    exec: &mut PgConnection,
    parent: &str,
    partition: &str,
    day: NaiveDate,
) -> Result<()> {
    // Parent and partition names come from compile-time constants or the
    // controlled `partition_name` builder; bounds come from a validated date.
    // PostgreSQL cannot bind identifiers, so the DDL is formatted safely.
    let ddl = format!(
        "CREATE TABLE IF NOT EXISTS {partition} PARTITION OF {parent} FOR VALUES FROM ('{}') TO ('{}')",
        day.format("%Y-%m-%d 00:00:00+00"),
        (day + ChronoDuration::days(1)).format("%Y-%m-%d 00:00:00+00"),
    );
    sqlx::query(sqlx::AssertSqlSafe(ddl)).execute(exec).await?;
    Ok(())
}

/// Catalog-only drop of one whole-day partition: no rows are deleted.
pub(crate) async fn drop_partition(conn: &mut PgConnection, partition: &str) -> Result<()> {
    let ddl = format!("DROP TABLE IF EXISTS {partition}");
    sqlx::query(sqlx::AssertSqlSafe(ddl)).execute(conn).await?;
    Ok(())
}

/// `<parent>_<YYYYMMDD>` for a managed parent.
pub(crate) fn partition_name(parent: &str, day: NaiveDate) -> String {
    format!("{parent}_{}", day.format("%Y%m%d"))
}

/// Inverse of [`partition_name`]: recover the day from a partition of a
/// managed parent. Unknown names (default partitions, foreign children) are
/// skipped by returning `None`.
pub(crate) fn partition_day(partition: &str, parent: &str) -> Option<NaiveDate> {
    let day_text = partition.strip_prefix(parent)?.strip_prefix('_')?;
    NaiveDate::parse_from_str(day_text, "%Y%m%d").ok()
}

/// A partition is droppable when its whole day ended before the retention
/// cutoff, and never when the day is the current UTC day or later.
pub(crate) fn partition_is_droppable(day: NaiveDate, cutoff: NaiveDate, today: NaiveDate) -> bool {
    day < cutoff && day < today
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_names_round_trip_through_the_day_parser() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 24).expect("valid date");
        let name = partition_name("request_records", day);
        assert_eq!(name, "request_records_20260924");
        assert_eq!(partition_day(&name, "request_records"), Some(day));
    }

    #[test]
    fn foreign_and_default_names_are_not_partition_days() {
        assert_eq!(
            partition_day("request_records_default", "request_records"),
            None
        );
        assert_eq!(
            partition_day("request_records_not_a_date", "request_records"),
            None
        );
        assert_eq!(
            partition_day("usage_prompt_blocks_20260924", "request_records"),
            None
        );
    }

    #[test]
    fn droppable_days_respect_the_cutoff_and_the_current_day() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 24).expect("valid date");
        let yesterday = today - ChronoDuration::days(1);

        // Retention cutoff: yesterday is inside the window and survives.
        assert!(!partition_is_droppable(
            yesterday,
            today - ChronoDuration::days(3),
            today
        ));
        assert!(partition_is_droppable(
            today - ChronoDuration::days(4),
            today - ChronoDuration::days(3),
            today
        ));

        // Full-history clear passes the current day as the cutoff, so
        // yesterday drops while today is always protected.
        assert!(partition_is_droppable(yesterday, today, today));
        assert!(!partition_is_droppable(today, today, today));
    }
}
