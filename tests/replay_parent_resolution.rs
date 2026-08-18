#[path = "support/db_harness.rs"]
mod db_harness;

use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db;
use uuid::Uuid;

fn completed_record(
    conversation_id: Uuid,
    parent_event_id: Option<i64>,
    conversation_seq: i32,
) -> db::RequestRecordCreate {
    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        )
        .with_request_actor(None, None, None, None);
    record.model = Some("minimax-m3".to_string());
    record.requested_model = Some("minimax-m3".to_string());
    record.conversation_id = Some(conversation_id);
    record.parent_event_id = parent_event_id;
    record.conversation_seq = Some(conversation_seq);
    record.conversation_source = "codex_thread_key".to_string();
    record
}

#[tokio::test]
async fn finds_same_conversation_tool_call_when_parent_chain_is_stale() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let conversation_id = Uuid::new_v4();
    let first_event =
        db::record_request_record(&schema.pool, completed_record(conversation_id, None, 1)).await?;
    let tool_parent_event = db::record_request_record(
        &schema.pool,
        completed_record(conversation_id, Some(first_event), 2),
    )
    .await?;
    let call_id = "call_stale_parent".to_string();

    db::upsert_usage_assistant_artifact(
        &schema.pool,
        db::UsageAssistantArtifactCreate {
            event_id: tool_parent_event,
            message_json: serde_json::json!({
                "version": 1,
                "assistant_message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "fc_1",
                        "type": "function",
                        "function": {"name": "read", "arguments": "{}"}
                    }]
                },
                "output_items": [{
                    "type": "reasoning",
                    "content": [{"type": "reasoning_text", "text": "inspect"}]
                }, {
                    "type": "function_call",
                    "call_id": call_id,
                    "name": "read",
                    "arguments": "{}"
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
            parent_event_id: tool_parent_event,
            conversation_id: Some(conversation_id),
            call_id: call_id.clone(),
            tool_name: "read".to_string(),
            arguments_json: Some(serde_json::json!({})),
            arguments_preview: Some("{}".to_string()),
            status: db::RequestToolCallStatus::Emitted,
            sequence_in_turn: Some(0),
            mcp_request_event_id: None,
        },
    )
    .await?;

    let candidates = db::find_request_record_tool_calls_by_call_ids(
        &schema.pool,
        &[call_id],
        None,
        None,
        Some(conversation_id),
        Some(first_event),
    )
    .await?;

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].tool_call.parent_event_id, tool_parent_event);
    assert!(candidates[0].has_assistant_artifact);

    schema.cleanup().await?;
    Ok(())
}
