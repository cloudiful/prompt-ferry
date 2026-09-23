#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::Utc;
use prompt_ferry::db;
use serde_json::json;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

fn completed_record() -> db::RequestRecordCreate {
    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        )
        .with_request_actor(None, None, None, None);
    record.request_full_json = Some(Box::new(json!([
        {"role": "user", "block_hash": "p7-content-expiry-block"}
    ])));
    record.response_prompt = Some("response prompt text".to_string());
    record.upstream_error_body = Some("upstream error text".to_string());
    record
}

fn list_query() -> db::RequestRecordQuery {
    db::RequestRecordQuery {
        visible_user_id: None,
        request_category: db::RequestRecordCategory::Ai,
        first: 0,
        rows: 20,
        sort_field: "created_at".to_string(),
        sort_order: -1,
        search: None,
        date_start: None,
        date_end: None,
        client_key_id: None,
        user: None,
        model: None,
        endpoint_id: None,
        mcp_server_id: None,
        mcp_bearer_token_slot: None,
        request_state: None,
        redaction_applied: None,
    }
}

async fn delete_content_row(
    pool: &sqlx::PgPool,
    event_id: i64,
    created_at: chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    // What the partition drop does later: the metadata row survives, the
    // content row is gone.
    sqlx::query("DELETE FROM request_record_content WHERE event_id = $1 AND created_at = $2")
        .bind(event_id)
        .bind(created_at)
        .execute(pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn missing_content_row_reads_as_expired_without_changing_metadata() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let record = completed_record();
    let created_at = record.created_at;
    let event_id = db::record_request_record(&schema.pool, record).await?;

    let live = db::get_visible_usage_event_detail(&schema.pool, event_id, None)
        .await?
        .expect("detail row exists before expiry");
    assert!(live.has_full_request);
    assert_eq!(
        live.response_prompt.as_deref(),
        Some("response prompt text")
    );
    assert_eq!(
        live.upstream_error_body.as_deref(),
        Some("upstream error text")
    );
    assert!(
        db::get_usage_event_chain_entry(&schema.pool, event_id)
            .await?
            .is_some(),
        "the replay path accepts a record while its content row is present"
    );

    delete_content_row(&schema.pool, event_id, created_at).await?;

    // Detail: metadata stays byte-identical, content columns fall back to null.
    let expired = db::get_visible_usage_event_detail(&schema.pool, event_id, None)
        .await?
        .expect("metadata row survives content expiry");
    assert_eq!(expired.record_id, event_id);
    // PostgreSQL keeps microseconds; the in-memory value is nanosecond-precise.
    assert_eq!(
        expired.created_at.timestamp_micros(),
        created_at.timestamp_micros()
    );
    assert!(!expired.has_full_request);
    assert_eq!(expired.response_prompt, None);
    assert_eq!(expired.upstream_error_body, None);

    // Replay and visible chains follow the same signal.
    assert!(
        db::get_usage_event_chain_entry(&schema.pool, event_id)
            .await?
            .is_none(),
        "the replay path rejects a record whose content row is gone"
    );
    let visible = db::get_visible_usage_event_chain_entry(&schema.pool, event_id, None)
        .await?
        .expect("the visible chain entry still resolves for display");
    assert_eq!(visible.request_full_json, None);
    assert_eq!(visible.request_delta_json, None);
    assert_eq!(visible.response_prompt, None);

    // List stays metadata-only and reports no request payload.
    let page = db::list_request_records(&schema.pool, list_query()).await?;
    let row = page
        .records
        .iter()
        .find(|row| row.record_id == event_id)
        .expect("expired record still lists");
    assert!(!row.has_full_request);

    schema.cleanup().await?;
    Ok(())
}
