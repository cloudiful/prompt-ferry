//! Issue #277 Phase P8: content-family retention is now partition-drop
//! driven, so the "expired content + children are deleted" behavior of the
//! legacy batch prune is covered by the partition-manager regression tests
//! (`tests/partition_maintenance.rs`). This file keeps the write-path
//! guarantee that tool calls carry the parent's `created_at` and therefore
//! land in the same daily partition as their request record.

#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

#[path = "usage_content_retention_write_path.rs"]
mod write_path_support;

#[tokio::test]
async fn tool_call_rows_share_the_parent_partition_day() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let (event_id, created_at) =
        write_path_support::create_completed_record_with_children(&schema.pool).await?;

    let remaining = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_tool_calls.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(remaining.count, 1, "the tool call must exist after insert");

    let parent_day = sqlx::query_scalar::<_, chrono::NaiveDateTime>(
        "SELECT created_at AT TIME ZONE 'UTC' FROM request_records WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&schema.pool)
    .await?;
    let child_day = sqlx::query_scalar::<_, chrono::NaiveDateTime>(
        "SELECT created_at AT TIME ZONE 'UTC' FROM request_record_tool_calls WHERE parent_event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        parent_day.date(),
        child_day.date(),
        "tool call rows must share the parent request record's partition day"
    );

    let _ = created_at;
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn expired_partition_drop_removes_content_family_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let (event_id, _created_at) =
        write_path_support::create_completed_record_with_children(&schema.pool).await?;

    // Move the metadata row (and its content-family children, which share the
    // same day by construction) to a droppable content-family day: inside the
    // P7 handover window (today-8..today+8) and past the 3-day content
    // horizon. The 90-day metadata horizon keeps the metadata row.
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        event_id,
        Utc::now() - Duration::days(5),
    )
    .execute(&schema.pool)
    .await?;

    // The content-family children were written "today"; move them onto the
    // same historical day, which is what a real expired event looks like.
    sqlx::query(
        "UPDATE request_record_content SET created_at = (SELECT created_at FROM request_records WHERE event_id = $1) WHERE event_id = $1",
    )
    .bind(event_id)
    .execute(&schema.pool)
    .await?;
    sqlx::query(
        "UPDATE request_record_tool_calls SET created_at = (SELECT created_at FROM request_records WHERE event_id = $1) WHERE parent_event_id = $1",
    )
    .bind(event_id)
    .execute(&schema.pool)
    .await?;
    sqlx::query(
        "UPDATE request_record_assistant_artifacts SET created_at = (SELECT created_at FROM request_records WHERE event_id = $1) WHERE event_id = $1",
    )
    .bind(event_id)
    .execute(&schema.pool)
    .await?;

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");
    assert!(
        report.partitions_dropped >= 6,
        "the expired content-family day's partitions must drop, got {}",
        report.partitions_dropped
    );

    let metadata = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        metadata.count, 1,
        "metadata survives its 90-day horizon while the content family expires"
    );

    let content = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM request_record_content WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        content, 0,
        "the expired content row leaves with its partition"
    );

    let tool_calls = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_tool_calls.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        tool_calls.count, 0,
        "expired tool-call rows leave with their partition"
    );

    schema.cleanup().await?;
    Ok(())
}
