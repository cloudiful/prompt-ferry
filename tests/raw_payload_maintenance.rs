#[path = "support/db_harness.rs"]
mod db_harness;

use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

/// Raw payload rows are metadata-only after migration 0066: expired per-event
/// objects are removed by dropping complete expired partitions while the main
/// request-record conversation metadata is cleared first.
#[tokio::test]
async fn drops_complete_expired_partition_after_clearing_record_metadata() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    sqlx::query_file!("tests/sql/raw_payloads_create_expired_partition.sql")
        .execute(&schema.pool)
        .await?;

    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    record.request_conversation_key = Some("expired-conversation".to_string());
    let event_id = db::record_request_record(&schema.pool, record).await?;
    attach_raw_payload_metadata(&schema.pool, event_id).await?;
    sqlx::query_file!(
        "tests/sql/raw_payloads_move_to_expired_partition.sql",
        event_id
    )
    .execute(&schema.pool)
    .await?;

    let report = db::run_raw_payload_maintenance(&schema.pool, 1)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert_eq!(report.raw_rows_deleted, 0);
    assert!(report.partitions_dropped >= 1);

    let remaining = sqlx::query_file!("tests/sql/raw_payloads_count_by_event.sql", event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(remaining.count, Some(0));

    let key = sqlx::query_file!("tests/sql/raw_payloads_conversation_key.sql", event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert!(key.request_conversation_key.is_none());

    schema.cleanup().await?;
    Ok(())
}

/// Expired metadata rows outside any live partition are batch-pruned and the
/// matching conversation metadata on the main record is cleared.
#[tokio::test]
async fn prunes_expired_raw_payload_metadata_without_bodies() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    record.request_conversation_key = Some("metadata-conversation".to_string());
    let event_id = db::record_request_record(&schema.pool, record).await?;
    attach_raw_payload_metadata(&schema.pool, event_id).await?;

    let initial_report = db::run_raw_payload_maintenance(&schema.pool, 3)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert!(initial_report.partitions_created > 0);

    sqlx::query_file!("tests/sql/raw_payloads_mark_expired.sql", event_id)
        .execute(&schema.pool)
        .await?;
    let report = db::run_raw_payload_maintenance(&schema.pool, 1)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert_eq!(report.raw_rows_deleted, 1);

    let pruned = sqlx::query_file!("tests/sql/raw_payloads_count_by_event.sql", event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(pruned.count, Some(0));
    let key = sqlx::query_file!("tests/sql/raw_payloads_conversation_key.sql", event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert!(key.request_conversation_key.is_none());

    schema.cleanup().await?;
    Ok(())
}

async fn attach_raw_payload_metadata(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<()> {
    sqlx::query_file!(
        "src/sql/usage/upsert_request_record_raw_object.sql",
        event_id,
        format!("prompt-ferry/raw/events/{event_id}.bin"),
        16_i64,
        "ab".repeat(32),
        chrono::Utc::now() + chrono::Duration::days(3),
    )
    .execute(pool)
    .await?;
    Ok(())
}
