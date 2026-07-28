#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_active_request(
    pool: &sqlx::PgPool,
    request_id: Uuid,
    lease_expires_at: chrono::DateTime<Utc>,
) -> anyhow::Result<i64> {
    let event_id = db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(request_id, "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::UpstreamProcessing,
            )
            .with_worker_lease(
                Some(Uuid::new_v4()),
                Some(lease_expires_at),
                Some(Utc::now()),
            ),
    )
    .await?;
    Ok(event_id)
}

async fn assert_aborted_record(
    pool: &sqlx::PgPool,
    event_id: i64,
    request_id: Uuid,
    expected_message: &str,
) -> anyhow::Result<()> {
    let row = db::get_visible_usage_event_detail(pool, event_id, None)
        .await?
        .expect("request record");
    assert_eq!(row.request_id, request_id);
    assert_eq!(row.request_state, db::RequestRecordState::Aborted);
    assert_eq!(row.ok, Some(false));
    assert_eq!(row.error_code.as_deref(), Some("request_aborted"));
    assert_eq!(row.error_message.as_deref(), Some(expected_message));
    let lease = sqlx::query_as::<_, (i64,)>(include_str!(
        "sql/usage_maintenance/count_request_record_leases.sql"
    ))
    .bind(request_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(lease.0, 0);
    Ok(())
}

#[tokio::test]
async fn client_cancellation_records_an_aborted_reason() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping request lease diagnostic test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let request_id = Uuid::new_v4();
    let event_id =
        create_active_request(&schema.pool, request_id, Utc::now() + Duration::minutes(5)).await?;

    assert_eq!(
        db::abort_request_record(
            &schema.pool,
            request_id,
            "request cancelled by downstream client before completion (relay reason: request_cancelled)",
        )
        .await?,
        1
    );
    assert_aborted_record(
        &schema.pool,
        event_id,
        request_id,
        "request cancelled by downstream client before completion (relay reason: request_cancelled)",
    )
    .await?;
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn worker_disconnect_is_recorded_as_lease_expiration() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping request lease diagnostic test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let request_id = Uuid::new_v4();
    let event_id =
        create_active_request(&schema.pool, request_id, Utc::now() - Duration::minutes(5)).await?;

    assert_eq!(db::abort_stale_request_records(&schema.pool).await?, 1);
    assert_aborted_record(
        &schema.pool,
        event_id,
        request_id,
        "request worker lease expired before completion; worker may have stopped or missed heartbeats",
    )
    .await?;
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn missing_valkey_lease_is_recorded_separately() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping request lease diagnostic test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let request_id = Uuid::new_v4();
    let event_id =
        create_active_request(&schema.pool, request_id, Utc::now() + Duration::minutes(5)).await?;

    assert_eq!(
        db::abort_request_records_by_ids(&schema.pool, &[request_id]).await?,
        1
    );
    assert_aborted_record(
        &schema.pool,
        event_id,
        request_id,
        "request Valkey lease was missing before completion",
    )
    .await?;
    schema.cleanup().await?;
    Ok(())
}
