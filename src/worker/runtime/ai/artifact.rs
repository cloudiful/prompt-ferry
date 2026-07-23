use crate::{
    chat_replay::{AssistantArtifact, fallback_text_artifact},
    db,
    openai_compat::persisted_output_items,
    usage::truncate_chars,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::AdminState,
};
use serde_json::Value;
use tracing::warn;

pub(super) fn resolve_assistant_artifact(
    captured: Option<AssistantArtifact>,
    raw_response_text: Option<&str>,
    logged_response_text: Option<&str>,
) -> Option<AssistantArtifact> {
    captured
        .or_else(|| raw_response_text.and_then(fallback_text_artifact))
        .or_else(|| logged_response_text.and_then(fallback_text_artifact))
}

fn tool_call_preview(arguments: &str) -> Option<String> {
    let trimmed = arguments.trim();
    (!trimmed.is_empty()).then(|| truncate_chars(trimmed, 240))
}

fn collect_tool_calls_from_artifact(
    artifact: &AssistantArtifact,
) -> Vec<db::RequestRecordToolCallCreate> {
    let mut calls = Vec::new();

    if let Ok(output_items) = persisted_output_items(&artifact.message_json) {
        for (index, item) in output_items.iter().enumerate() {
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                continue;
            }
            let Some(call_id) = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(tool_name) = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            calls.push(db::RequestRecordToolCallCreate {
                parent_event_id: 0,
                conversation_id: None,
                call_id: call_id.to_string(),
                tool_name: tool_name.to_string(),
                arguments_json: serde_json::from_str(arguments).ok(),
                arguments_preview: tool_call_preview(arguments),
                status: db::RequestToolCallStatus::Emitted,
                sequence_in_turn: Some(index as i32),
                mcp_request_event_id: None,
            });
        }
    }

    calls
}

async fn persist_tool_call_events(
    admin_state: Option<&AdminState>,
    parent_event_id: i64,
    conversation_id: Option<uuid::Uuid>,
    tool_calls: Vec<db::RequestRecordToolCallCreate>,
) {
    let Some(state) = admin_state else {
        return;
    };
    for tool_call in tool_calls {
        if let Err(err) = db::upsert_request_record_tool_call(
            &state.pool,
            db::RequestRecordToolCallCreate {
                parent_event_id,
                conversation_id,
                ..tool_call
            },
        )
        .await
        {
            warn!(error = %err, parent_event_id, "failed to persist tool call child event");
        }
    }
}

pub(super) async fn persist_assistant_artifact(
    admin_state: Option<&AdminState>,
    usage_event_id: Option<i64>,
    artifact: Option<AssistantArtifact>,
    conversation_id: Option<uuid::Uuid>,
    request: &BufferedBridgeRequest,
    route: &db::RouteConfig,
    provider_response_id: Option<&str>,
) {
    let (Some(state), Some(event_id)) = (admin_state, usage_event_id) else {
        return;
    };
    let Some(artifact) = artifact else {
        warn!(
            request_id = %request.request_id,
            event_id,
            provider_response_id = provider_response_id.unwrap_or(""),
            path = %request.path,
            endpoint_id = %route.route_id,
            native_api = %route.native_api.as_str(),
            "assistant replay artifact unavailable after fallback recovery"
        );
        return;
    };
    let tool_calls = collect_tool_calls_from_artifact(&artifact);
    match db::upsert_usage_assistant_artifact(
        &state.pool,
        db::UsageAssistantArtifactCreate {
            event_id,
            message_json: artifact.message_json,
            has_reasoning_content: artifact.has_reasoning_content,
            has_tool_calls: artifact.has_tool_calls,
        },
    )
    .await
    {
        Err(err) => {
            warn!(error = %err, event_id, "failed to persist assistant replay artifact");
        }
        Ok(stats) if stats.sanitized() => {
            warn!(
                request_id = %request.request_id,
                event_id,
                nul_count = i32::try_from(stats.nul_count).unwrap_or(i32::MAX),
                "sanitized NUL bytes from assistant artifact storage payload before postgres write"
            );
        }
        Ok(_) => {}
    }
    persist_tool_call_events(Some(state), event_id, conversation_id, tool_calls).await;
}
