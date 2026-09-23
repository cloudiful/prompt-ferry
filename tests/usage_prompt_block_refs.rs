#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use serde_json::json;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

/// Mirrors the orphan-cleanup grace window: the delete pass must never touch a
/// prompt block that is still young enough to belong to an in-flight request.
const ORPHAN_GRACE_MINUTES: i64 = 30;

#[tokio::test]
async fn in_flight_prompt_block_survives_orphan_cleanup_inside_grace_window() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let in_flight_hash = "p5-in-flight-prompt-block";
    let stale_hash = "p5-stale-orphan-prompt-block";
    db::upsert_usage_prompt_block(
        &schema.pool,
        in_flight_hash,
        "user",
        &json!({"role": "user", "content": "in flight"}),
        "in flight",
    )
    .await?;
    db::upsert_usage_prompt_block(
        &schema.pool,
        stale_hash,
        "user",
        &json!({"role": "user", "content": "stale"}),
        "stale",
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_usage_prompt_block_created_at.sql",
        stale_hash,
        Utc::now() - Duration::minutes(ORPHAN_GRACE_MINUTES + 1),
    )
    .execute(&schema.pool)
    .await?;

    let report = db::run_usage_content_maintenance(&schema.pool, 1)
        .await?
        .expect("content maintenance should acquire its advisory lock");

    let in_flight = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_usage_prompt_blocks.sql",
        in_flight_hash,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        in_flight.count, 1,
        "a prompt block created inside the grace window must not be collected as an orphan"
    );
    let stale = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_usage_prompt_blocks.sql",
        stale_hash,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        stale.count, 0,
        "an orphan older than the grace window is collected"
    );
    assert_eq!(report.orphan_prompt_blocks_deleted, 1);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn dangling_block_ref_does_not_fail_usage_record_write() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let existing_hash = "p5-existing-prompt-block";
    db::upsert_usage_prompt_block(
        &schema.pool,
        existing_hash,
        "user",
        &json!({"role": "user", "content": "kept"}),
        "kept",
    )
    .await?;

    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        );
    record.request_full_json = Some(json!([
        {"role": "user", "block_hash": existing_hash},
        {"role": "user", "block_hash": "p5-block-that-no-longer-exists"},
    ]));

    let event_id = db::record_request_record(&schema.pool, record)
        .await
        .expect("a dangling prompt block ref must not fail the usage record write");

    let stored = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(stored.count, 1);
    let refs = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_block_refs.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        refs.count, 1,
        "only refs whose prompt block still exists are inserted"
    );

    schema.cleanup().await?;
    Ok(())
}
