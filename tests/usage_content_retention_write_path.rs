//! Shared fixture for partition-retention regression tests: a completed
//! request record plus its content-family children (artifact + tool call).

use prompt_ferry::db;

pub async fn create_completed_record_with_children(
    pool: &sqlx::PgPool,
) -> anyhow::Result<(i64, chrono::DateTime<chrono::Utc>)> {
    let record = db::RequestRecordCreate::ai_request(uuid::Uuid::new_v4(), "/v1/responses")
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        )
        .with_request_actor(Some(1), None, None, None);
    let created_at = record.created_at;
    let event_id = db::record_request_record(pool, record).await?;
    db::upsert_usage_assistant_artifact(
        pool,
        db::UsageAssistantArtifactCreate {
            event_id,
            created_at,
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
        pool,
        db::RequestRecordToolCallCreate {
            created_at,
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
    Ok((event_id, created_at))
}
