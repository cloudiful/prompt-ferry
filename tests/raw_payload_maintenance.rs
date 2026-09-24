#[path = "support/db_harness.rs"]
mod db_harness;

use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

/// Raw payload rows are metadata-only after migration 0066: expired per-event
/// objects are removed by dropping complete expired partitions. Since Phase
/// P8 the DROP itself belongs to the shared partition manager, so this test
/// drives both halves the way the runtime tick does.
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

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 1,
        content_retention_days: 1,
    };
    let partitions = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");
    assert!(partitions.partitions_dropped >= 1);

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

/// Issue #277 Phase P11: the raw tick used to finish with a whole-family
/// `VACUUM (ANALYZE)` of `request_records` and `request_record_raw_payloads`
/// (99+ partitions, ~11s per tick in production). Statistics now belong to
/// the shared partition manager's per-partition analyze of freshly created
/// partitions and to autovacuum for everything else, so a partition the raw
/// tick did not create keeps its never-analyzed sentinel.
#[tokio::test]
async fn tick_does_not_mass_analyze_the_partition_family() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    // Re-create a forward metadata partition so the fixture starts with the
    // never-analyzed sentinel regardless of the migration's ANALYZE.
    let today = chrono::Utc::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);
    let partition = format!("request_records_{}", tomorrow.format("%Y%m%d"));
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS {partition}"
    )))
    .execute(&schema.pool)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE {partition} PARTITION OF request_records FOR VALUES FROM ('{}') TO ('{}')",
        tomorrow.format("%Y-%m-%d 00:00:00+00"),
        (tomorrow + chrono::Duration::days(1)).format("%Y-%m-%d 00:00:00+00"),
    )))
    .execute(&schema.pool)
    .await?;
    let before = partition_reltuples(&schema.pool, &partition).await?;
    assert_eq!(before, -1, "a fresh partition starts with reltuples = -1");

    let report = db::run_raw_payload_maintenance(&schema.pool, 3)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert!(
        report.partitions_created > 0,
        "the raw tick still creates its own partitions"
    );

    let after = partition_reltuples(&schema.pool, &partition).await?;
    assert_eq!(
        after, -1,
        "the raw tick must not sweep the whole family with VACUUM (ANALYZE)"
    );

    schema.cleanup().await?;
    Ok(())
}

/// Issue #277 Phase P11: the raw helper's own create path (today..+7, wider
/// than the shared manager's +3) must ANALYZE every partition it creates, so
/// a fresh partition never waits for autoanalyze to get `reltuples`. This is
/// the gap reviewer note 8261 flagged: the +4..+7 days are created only here.
#[tokio::test]
async fn raw_helper_analyzes_the_partitions_it_creates() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let report = db::run_raw_payload_maintenance(&schema.pool, 3)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert_eq!(
        report.partitions_created, 8,
        "a fresh schema has no raw-payload day partition, so the helper creates today..+7"
    );

    let today = chrono::Utc::now().date_naive();
    for offset in 0..=7i64 {
        let day = today + chrono::Duration::days(offset);
        let partition = format!("request_record_raw_payloads_{}", day.format("%Y%m%d"));
        let reltuples = partition_reltuples(&schema.pool, &partition).await?;
        assert!(
            reltuples >= 0,
            "raw helper-created partition {partition} must be ANALYZEd, reltuples = {reltuples}"
        );
    }

    schema.cleanup().await?;
    Ok(())
}

async fn partition_reltuples(pool: &sqlx::PgPool, partition: &str) -> anyhow::Result<i64> {
    let row = sqlx::query_file!(
        "tests/sql/usage_maintenance/partition_reltuples.sql",
        partition,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.reltuples)
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
