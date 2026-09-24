//! Issue #277 Phase P8: metadata retention is partition-DROP based. The old
//! row-wise prune semantics (billing protection, lease protection) are gone
//! by operator decision: expired days leave wholesale. Admin clear semantics
//! are covered by `partition_clear.rs`.

#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_record(
    pool: &sqlx::PgPool,
    request_id: Uuid,
    user_id: Option<i64>,
    state: db::RequestRecordState,
) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(request_id, "/v1/responses")
            .with_state(db::UsageEventKind::Request, state)
            .with_request_actor(user_id, None, None, None),
    )
    .await
}

/// "Expired" in the partition model means the row's whole UTC day is older
/// than the metadata retention horizon. The content row is deleted first:
/// content-family days exist only for the P7 handover window
/// (today-8..today+8), so a metadata-expired row's content expired long
/// before ("content row absent" is the expired signal).
async fn mark_record_expired(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM request_record_content WHERE event_id = $1")
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn expired_metadata_partition_drops_and_recent_partition_survives() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let expired_event = create_record(
        &schema.pool,
        Uuid::new_v4(),
        None,
        db::RequestRecordState::Completed,
    )
    .await?;
    let charged_event = create_record(
        &schema.pool,
        Uuid::new_v4(),
        None,
        db::RequestRecordState::Completed,
    )
    .await?;
    mark_record_expired(&schema.pool, expired_event).await?;
    mark_record_expired(&schema.pool, charged_event).await?;

    // The metadata day that these rows were moved to must exist: move both
    // rows onto a historical day inside the P7 handover window (day -92 is
    // beyond the 90-day retention but inside today-95..today+8).
    let historical_day = Utc::now() - Duration::days(92);
    for event_id in [expired_event, charged_event] {
        sqlx::query_file!(
            "tests/sql/usage_maintenance/set_request_record_created_at.sql",
            event_id,
            historical_day,
        )
        .execute(&schema.pool)
        .await?;
    }

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");
    assert!(report.partitions_dropped >= 1);

    let expired = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        expired_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        expired.count, 0,
        "an expired day's rows leave with the partition"
    );

    // The billed row sat in the same expired day, so it is gone too: billing
    // protection no longer holds in the partition-drop model.
    let charged = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        charged_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        charged.count, 0,
        "a row in an expired day leaves with the partition even if later billed"
    );

    let recent = create_record(
        &schema.pool,
        Uuid::new_v4(),
        None,
        db::RequestRecordState::Completed,
    )
    .await?;
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("second partition maintenance round should acquire its lock");
    assert_eq!(
        report.partitions_dropped, 0,
        "a second round with nothing new expired must not drop today's partition"
    );
    let kept = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        recent,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(kept.count, 1, "today's rows are never dropped");

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn partition_maintenance_skips_when_lock_is_held() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let lock_key = db::PARTITION_MAINTENANCE_LOCK_KEY;
    let mut lock_connection = schema.pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "tests/sql/usage_maintenance/try_acquire_partition_lock.sql",
        lock_key,
    )
    .fetch_one(&mut *lock_connection)
    .await?;
    assert!(acquired);

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let skipped = db::run_partition_maintenance(&schema.pool, horizons).await?;
    assert!(skipped.is_none());

    let released = sqlx::query_file_scalar!(
        "tests/sql/usage_maintenance/release_partition_lock.sql",
        lock_key,
    )
    .fetch_one(&mut *lock_connection)
    .await?;
    assert!(released);
    drop(lock_connection);

    assert!(
        db::run_partition_maintenance(&schema.pool, horizons)
            .await?
            .is_some()
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn approval_retention_keeps_pending_and_recent_resolved_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let create = |status: &str| db::ApprovalRequestCreate {
        approval_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        user_id: None,
        client_key_label: None,
        path: "/v1/responses".to_string(),
        model: Some("test-model".to_string()),
        review_decision: "flag".to_string(),
        approval_status: status.to_string(),
        review_reason: "test".to_string(),
        review_categories: Vec::new(),
        request_preview: "test".to_string(),
        request_payload_json: None,
        request_deadline_unix_ms: 0,
        wait_deadline_unix_ms: 0,
    };
    let pending = db::create_approval_request(&schema.pool, create("pending")).await?;
    let expired = db::create_approval_request(&schema.pool, create("approved")).await?;
    let recent = db::create_approval_request(&schema.pool, create("rejected")).await?;
    let old = Utc::now() - Duration::days(30);
    for approval_id in [pending.approval_id, expired.approval_id] {
        sqlx::query_file!(
            "tests/sql/usage_maintenance/set_approval_created_at.sql",
            approval_id,
            old,
        )
        .execute(&schema.pool)
        .await?;
    }

    let first = db::run_approval_retention_maintenance(&schema.pool, 1)
        .await?
        .expect("approval retention should acquire its advisory lock");
    let second = db::run_approval_retention_maintenance(&schema.pool, 1)
        .await?
        .expect("approval retention should release its advisory lock");
    assert_eq!(first + second, 1);

    for (approval_id, expected) in [
        (pending.approval_id, 1),
        (expired.approval_id, 0),
        (recent.approval_id, 1),
    ] {
        let count = sqlx::query_file!(
            "tests/sql/usage_maintenance/count_approval.sql",
            approval_id,
        )
        .fetch_one(&schema.pool)
        .await?;
        assert_eq!(count.count, expected);
    }

    schema.cleanup().await?;
    Ok(())
}
