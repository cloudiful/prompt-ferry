//! Issue #657 Phase P1: the first meaningful output instant is stamped on the
//! running request record immediately, exactly once, and never overwrites a
//! value recorded earlier or a terminal write that follows it.

#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn record_with_state(
    pool: &sqlx::PgPool,
    request_id: Uuid,
    created_at: chrono::DateTime<Utc>,
    state: db::RequestRecordState,
    ttft_ms: Option<i64>,
) -> anyhow::Result<i64> {
    let create = db::RequestRecordCreate::ai_request(request_id, "/v1/responses")
        .with_state(db::UsageEventKind::Request, state)
        .with_created_at(created_at)
        .with_timing(
            Some(200),
            Some(state == db::RequestRecordState::Completed),
            Some(3_000),
            ttft_ms,
        );
    db::record_request_record(pool, create).await
}

async fn record_of(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<db::RequestRecordDetail> {
    Ok(db::get_visible_usage_event_detail(pool, event_id, None)
        .await?
        .expect("request record"))
}

#[tokio::test]
async fn first_output_is_recorded_once_and_survives_the_terminal_write() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping first output test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let request_id = Uuid::new_v4();
    let created_at = Utc::now();
    let event_id = record_with_state(
        &schema.pool,
        request_id,
        created_at,
        db::RequestRecordState::UpstreamProcessing,
        None,
    )
    .await?;

    assert_eq!(
        db::record_request_first_output(&schema.pool, request_id, Some(created_at), 120).await?,
        1
    );
    let row = record_of(&schema.pool, event_id).await?;
    assert_eq!(row.ttft_ms, Some(120));
    assert_eq!(
        row.request_state,
        db::RequestRecordState::UpstreamProcessing
    );

    // A repeated observation, or an upstream retry, never moves the instant.
    assert_eq!(
        db::record_request_first_output(&schema.pool, request_id, Some(created_at), 999).await?,
        1
    );
    // A partition instant that does not match the row updates nothing.
    assert_eq!(
        db::record_request_first_output(
            &schema.pool,
            request_id,
            Some(created_at + Duration::seconds(1)),
            999,
        )
        .await?,
        0
    );
    assert_eq!(record_of(&schema.pool, event_id).await?.ttft_ms, Some(120));

    // The terminal write keeps the recorded first output instant, and a late
    // first-output write can neither move it nor revive the running state.
    record_with_state(
        &schema.pool,
        request_id,
        created_at,
        db::RequestRecordState::Completed,
        None,
    )
    .await?;
    assert_eq!(
        db::record_request_first_output(&schema.pool, request_id, Some(created_at), 555).await?,
        1
    );
    let row = record_of(&schema.pool, event_id).await?;
    assert_eq!(row.request_state, db::RequestRecordState::Completed);
    assert_eq!(row.ok, Some(true));
    assert_eq!(row.duration_ms, Some(3_000));
    assert_eq!(row.ttft_ms, Some(120));

    schema.cleanup().await?;
    Ok(())
}
