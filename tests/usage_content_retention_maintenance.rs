#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_completed_record(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    Ok(db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(Some(1), None, None, None),
    )
    .await?)
}

#[tokio::test]
async fn content_retention_deletes_assistant_tool_call_children_with_artifacts()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let event_id = create_completed_record(&schema.pool).await?;
    db::upsert_usage_assistant_artifact(
        &schema.pool,
        db::UsageAssistantArtifactCreate {
            event_id,
            message_json: serde_json::json!({
                "role": "assistant",
                "reasoning_content": "internal steps",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {"name": "lookup", "arguments": "{}"}
                }]
            }),
            has_reasoning_content: true,
            has_tool_calls: true,
        },
    )
    .await?;
    db::upsert_request_record_tool_call(
        &schema.pool,
        db::RequestRecordToolCallCreate {
            parent_event_id: event_id,
            conversation_id: None,
            call_id: "call_1".to_string(),
            tool_name: "lookup".to_string(),
            arguments_json: Some(serde_json::json!({})),
            arguments_preview: Some("{}".to_string()),
            status: db::RequestToolCallStatus::Emitted,
            sequence_in_turn: Some(0),
            mcp_request_event_id: None,
        },
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        event_id,
        Utc::now() - Duration::days(30),
    )
    .execute(&schema.pool)
    .await?;

    let report = db::run_usage_content_maintenance(&schema.pool, 1)
        .await?
        .expect("content maintenance should acquire its advisory lock");

    assert_eq!(report.deleted_artifacts, 1);
    assert_eq!(report.deleted_tool_calls, 1);
    assert_eq!(report.cleared_tool_arguments, 1);
    let remaining = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_tool_calls.sql",
        event_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(remaining.count, 0);

    schema.cleanup().await?;
    Ok(())
}
