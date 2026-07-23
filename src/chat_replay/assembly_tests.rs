use std::collections::HashMap;

use serde_json::json;

use super::{AssistantArtifact, assembly::*};

#[test]
fn deepseek_gate_strips_reasoning_for_non_deepseek_routes() {
    let message = json!({
        "role": "assistant",
        "content": "hello",
        "reasoning_content": "hidden",
        "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {"name": "lookup", "arguments": "{}"}
        }]
    });

    let stripped = replay_assistant_message(&message, false).unwrap();
    assert!(stripped.get("reasoning_content").is_none());
    assert!(stripped.get("reasoning_details").is_none());
    assert_eq!(stripped["content"].as_str(), Some("hello"));
    assert_eq!(stripped["tool_calls"][0]["id"].as_str(), Some("call_1"));
}

#[test]
fn deepseek_gate_requires_history_and_signal() {
    let mut artifacts = HashMap::new();
    artifacts.insert(
        1,
        AssistantArtifact {
            message_json: json!({"role":"assistant","content":"hello","reasoning_content":"hidden"}),
            has_reasoning_content: true,
            has_tool_calls: false,
        },
    );

    assert!(should_replay_reasoning(
        Some("deepseek-chat"),
        None,
        "https://example.com",
        &artifacts
    ));
    assert!(should_replay_reasoning(
        None,
        Some("deepseek-reasoner"),
        "https://example.com",
        &artifacts
    ));
    assert!(should_replay_reasoning(
        None,
        None,
        "https://api.deepseek.com",
        &artifacts
    ));
    assert!(should_replay_reasoning(
        Some("MiniMax-M3"),
        None,
        "https://example.com",
        &artifacts
    ));
    assert!(should_replay_reasoning(
        None,
        None,
        "https://api.minimax.chat",
        &artifacts
    ));
    assert!(!should_replay_reasoning(
        Some("gpt-5"),
        Some("gpt-4"),
        "https://example.com",
        &artifacts
    ));
}

#[test]
fn rejects_phase_or_refusal_replay_semantics() {
    let phase_err = replay_assistant_message(
        &json!({"role":"assistant","content":"hello","phase":"analysis"}),
        true,
    )
    .unwrap_err();
    assert_eq!(phase_err.code, "replay_unavailable");
    assert!(phase_err.message.contains("phase"));

    let refusal_err = replay_assistant_message(
        &json!({"role":"assistant","content":null,"refusal":"nope"}),
        true,
    )
    .unwrap_err();
    assert_eq!(refusal_err.code, "replay_unavailable");
    assert!(refusal_err.message.contains("refusal"));
}

#[test]
fn filters_reasoning_items_from_responses_native_replay_prefix() {
    let items = vec![
        json!({
            "id": "rs_1",
            "type": "reasoning",
            "summary": []
        }),
        json!({
            "id": "fc_1",
            "type": "function_call",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        }),
        json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [{"type":"output_text","text":"done"}]
        }),
    ];

    let filtered = replayable_output_items(&items);
    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .all(|item| item["type"].as_str() != Some("reasoning"))
    );
    assert_eq!(filtered[0]["type"].as_str(), Some("function_call"));
    assert_eq!(filtered[1]["type"].as_str(), Some("message"));
}

#[test]
fn builds_fallback_artifact_from_response_prompt() {
    let entry = crate::db::UsageEventChainEntry {
        event_id: 1,
        request_id: uuid::Uuid::nil(),
        user_id: None,
        endpoint_id: None,
        path: "/v1/responses".to_string(),
        model: Some("gpt-test".to_string()),
        conversation_id: None,
        parent_event_id: None,
        conversation_seq: Some(1),
        conversation_source: "none".to_string(),
        client_installation_id: None,
        normalized_item_count: None,
        normalized_chain_hash: None,
        normalized_first_ref_hash: None,
        normalized_last_ref_hash: None,
        request_storage_mode: "full".to_string(),
        request_full_json: None,
        request_delta_json: None,
        request_raw_json: None,
        request_has_previous_response_id: false,
        request_previous_response_id: None,
        request_previous_response_parent_found: None,
        request_conversation_key: None,
        request_conversation_parent_found: None,
        provider_response_id: Some("resp_1".to_string()),
        base_checkpoint_event_id: None,
        response_prompt: Some("done".to_string()),
        response_raw_body: None,
    };

    let artifact = fallback_artifact_for_entry(&entry).unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["content"].as_str(),
        Some("done")
    );
}
