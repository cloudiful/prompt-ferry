use super::*;
use serde_json::json;

#[test]
fn groups_adjacent_responses_function_calls_into_one_assistant_turn() {
    let input = vec![
        json!({"role":"user","content":"check"}),
        json!({
            "type":"function_call",
            "call_id":"call_1",
            "name":"one",
            "arguments":"{}"
        }),
        json!({
            "type":"function_call",
            "call_id":"call_2",
            "name":"two",
            "arguments":"{\"value\":2}"
        }),
        json!({
            "type":"function_call_output",
            "call_id":"call_1",
            "output":"one"
        }),
    ];

    let groups = response_tool_call_groups(&input).unwrap();

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_index, 1);
    assert_eq!(groups[0].call_ids, ["call_1", "call_2"]);
    assert_eq!(
        groups[0].message["tool_calls"][1]["function"]["name"],
        "two"
    );
}

#[test]
fn recognizes_complete_reasoning_before_a_function_call_turn() {
    let input = vec![
        json!({"role":"user","content":"check"}),
        json!({
            "type":"reasoning",
            "summary":[{"type":"summary_text","text":"ignored"}],
            "content":[{"type":"reasoning_text","text":"full reasoning"}]
        }),
        json!({
            "type":"function_call",
            "call_id":"call_1",
            "name":"one",
            "arguments":"{}"
        }),
    ];
    let group = &response_tool_call_groups(&input).unwrap()[0];

    assert!(has_reasoning_for_group(&input, group.first_index));
}

#[test]
fn normalizes_artifact_reasoning_to_deepseek_input_shape() {
    let artifact = json!({
        "version": 1,
        "assistant_message": {
            "role":"assistant",
            "content":null,
            "reasoning_content":"fallback should not be used",
            "tool_calls":[]
        },
        "output_items": [{
            "type":"reasoning",
            "id":"reasoning-id",
            "status":"completed",
            "summary":[{"type":"summary_text","text":"unsupported"}],
            "content":[{"type":"reasoning_text","text":"full reasoning"}]
        }]
    });

    let item = reasoning_input_item(&artifact, false).unwrap();

    assert_eq!(
        item,
        json!({
            "type":"reasoning",
            "content":[{"type":"reasoning_text","text":"full reasoning"}]
        })
    );
    assert!(item.get("summary").is_none());
    assert!(item.get("id").is_none());
    assert!(item.get("status").is_none());
}

#[test]
fn rejects_artifact_without_reasoning_text() {
    let artifact = json!({
        "version": 1,
        "assistant_message": {"role":"assistant","content":null},
        "output_items": [{
            "type":"reasoning",
            "summary":[{"type":"summary_text","text":"only summary"}],
            "content":[]
        }]
    });

    let error = reasoning_input_item(&artifact, false).unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing reasoning_text"));
}

#[test]
fn uses_reasoning_details_as_a_responses_replay_fallback() {
    let artifact = json!({
        "version": 1,
        "assistant_message": {
            "role": "assistant",
            "content": null,
            "reasoning_details": [{
                "type": "reasoning.text",
                "id": "reasoning-text-1",
                "format": "MiniMax-response-v1",
                "index": 0,
                "text": "preserve this reasoning"
            }]
        },
        "output_items": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        }]
    });

    assert_eq!(
        reasoning_input_item(&artifact, true).unwrap(),
        json!({
            "type": "reasoning",
            "content": [{"type": "reasoning_text", "text": "preserve this reasoning"}]
        })
    );
}

#[test]
fn does_not_use_reasoning_details_for_non_minimax_responses_replay() {
    let artifact = json!({
        "version": 1,
        "assistant_message": {
            "role": "assistant",
            "content": null,
            "reasoning_details": [{"text": "must not be used"}]
        },
        "output_items": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        }]
    });

    let error = reasoning_input_item(&artifact, false).unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing reasoning_text"));
}

#[test]
fn optional_reasoning_item_allows_an_artifact_without_reasoning_text() {
    let artifact = json!({
        "version": 1,
        "assistant_message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "lookup", "arguments": "{}"}
            }]
        },
        "output_items": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": "{}"
        }]
    });

    assert_eq!(
        reasoning_input_item_optional(&artifact, false).unwrap(),
        None
    );
}
