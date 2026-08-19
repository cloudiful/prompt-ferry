use super::{ResponsesReplayToolCall, resolve_responses_replay_groups};
use serde_json::json;
use std::collections::HashMap;

fn call(input_index: usize, call_id: &str, name: &str) -> ResponsesReplayToolCall {
    ResponsesReplayToolCall {
        input_index,
        call_id: call_id.to_string(),
        client_executed: false,
        tool_call: json!({
            "id": call_id,
            "type": "function",
            "function": {"name": name, "arguments": "{}"}
        }),
    }
}

fn artifact(tool_calls: serde_json::Value) -> serde_json::Value {
    json!({
        "version": 1,
        "assistant_message": {
            "role": "assistant",
            "content": null,
            "tool_calls": tool_calls
        }
    })
}

#[test]
fn splits_adjacent_calls_from_different_historical_assistant_turns() {
    let calls = vec![
        call(2, "call_1", "one"),
        call(3, "call_2", "two"),
        call(4, "call_3", "three"),
        call(5, "call_4", "four"),
    ];
    let candidates = HashMap::from([
        ("call_1".to_string(), vec![(10, true)]),
        ("call_2".to_string(), vec![(11, true)]),
        ("call_3".to_string(), vec![(12, true)]),
        ("call_4".to_string(), vec![(12, true)]),
    ]);
    let artifacts = HashMap::from([
        (10, artifact(json!([calls[0].tool_call.clone()]))),
        (11, artifact(json!([calls[1].tool_call.clone()]))),
        (
            12,
            artifact(json!([
                calls[2].tool_call.clone(),
                calls[3].tool_call.clone()
            ])),
        ),
    ]);

    let groups = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect("historical turns should resolve independently");

    assert_eq!(
        groups
            .iter()
            .map(|group| (group.first_index, group.parent_event_id))
            .collect::<Vec<_>>(),
        [(2, 10), (3, 11), (4, 12)]
    );
}

#[test]
fn keeps_multiple_calls_from_one_assistant_turn_together() {
    let calls = vec![call(2, "call_1", "one"), call(3, "call_2", "two")];
    let candidates = HashMap::from([
        ("call_1".to_string(), vec![(10, true)]),
        ("call_2".to_string(), vec![(10, true)]),
    ]);
    let artifacts = HashMap::from([(
        10,
        artifact(json!([
            calls[0].tool_call.clone(),
            calls[1].tool_call.clone()
        ])),
    )]);

    let groups = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect("one multi-tool assistant turn should remain grouped");

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_index, 2);
    assert_eq!(groups[0].parent_event_id, 10);
}

#[test]
fn rejects_a_call_without_a_live_artifact() {
    let calls = vec![call(2, "call_1", "one")];
    let error = resolve_responses_replay_groups(&calls, &HashMap::new(), &HashMap::new(), true)
        .expect_err("a missing replay artifact must be rejected");

    assert_replay_failure(&error, "missing_artifact");
}

#[test]
fn rejects_multiple_parents_without_provenance() {
    let calls = vec![call(2, "call_1", "one")];
    let candidates = HashMap::from([("call_1".to_string(), vec![(10, true), (11, true)])]);
    let artifacts = HashMap::from([
        (10, artifact(json!([calls[0].tool_call.clone()]))),
        (11, artifact(json!([calls[0].tool_call.clone()]))),
    ]);
    let error = resolve_responses_replay_groups(&calls, &candidates, &artifacts, false)
        .expect_err("multiple parents without provenance must be rejected");

    assert_replay_failure(&error, "ambiguous_parent");
}

#[test]
fn rejects_a_tool_call_that_does_not_match_its_artifact() {
    let calls = vec![call(2, "call_1", "one")];
    let candidates = HashMap::from([("call_1".to_string(), vec![(10, true)])]);
    let artifacts = HashMap::from([(
        10,
        artifact(json!([call(2, "call_1", "different").tool_call])),
    )]);
    let error = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect_err("a mismatched replay artifact must be rejected");

    assert_replay_failure(&error, "signature_mismatch");
}

#[test]
fn groups_calls_with_interleaved_output_items_by_parent_only() {
    let calls = vec![
        call(12, "call_read_1", "read"),
        call(14, "call_glob", "glob"),
        call(16, "call_read_2", "read"),
    ];
    let candidates = HashMap::from([
        ("call_read_1".to_string(), vec![(682551, true)]),
        ("call_glob".to_string(), vec![(682551, true)]),
        ("call_read_2".to_string(), vec![(682551, true)]),
    ]);
    let artifacts = HashMap::from([(
        682551,
        artifact(json!([
            calls[0].tool_call.clone(),
            calls[1].tool_call.clone(),
            calls[2].tool_call.clone(),
        ])),
    )]);

    let groups = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect("calls under one parent should merge despite interleaved output items");

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_index, 12);
    assert_eq!(groups[0].parent_event_id, 682551);
}

#[test]
fn matches_a_three_call_artifact_from_a_single_parent() {
    let calls = vec![
        call(12, "call_read_a", "read"),
        call(14, "call_glob", "glob"),
        call(16, "call_read_b", "read"),
    ];
    let candidates = HashMap::from([
        ("call_read_a".to_string(), vec![(682551, true)]),
        ("call_glob".to_string(), vec![(682551, true)]),
        ("call_read_b".to_string(), vec![(682551, true)]),
    ]);
    let artifacts = HashMap::from([(
        682551,
        artifact(json!([
            calls[0].tool_call.clone(),
            calls[1].tool_call.clone(),
            calls[2].tool_call.clone(),
        ])),
    )]);

    let groups = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect("three calls under one parent should match the stored three-call artifact");

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_index, 12);
    assert_eq!(groups[0].parent_event_id, 682551);
}

#[test]
fn splits_calls_with_different_parents_even_when_input_indices_are_contiguous() {
    let calls = vec![
        call(12, "call_a", "one"),
        call(13, "call_b", "two"),
        call(14, "call_c", "three"),
    ];
    let candidates = HashMap::from([
        ("call_a".to_string(), vec![(10, true)]),
        ("call_b".to_string(), vec![(11, true)]),
        ("call_c".to_string(), vec![(12, true)]),
    ]);
    let artifacts = HashMap::from([
        (10, artifact(json!([calls[0].tool_call.clone()]))),
        (11, artifact(json!([calls[1].tool_call.clone()]))),
        (12, artifact(json!([calls[2].tool_call.clone()]))),
    ]);

    let groups = resolve_responses_replay_groups(&calls, &candidates, &artifacts, true)
        .expect("different parents must split into separate groups");

    assert_eq!(
        groups
            .iter()
            .map(|group| (group.first_index, group.parent_event_id))
            .collect::<Vec<_>>(),
        [(12, 10), (13, 11), (14, 12)]
    );
}

fn assert_replay_failure(error: &crate::openai_compat::CompatError, kind: &str) {
    assert_eq!(error.code, "replay_unavailable");
    assert!(
        error.message.starts_with(kind),
        "expected {kind} replay failure, got {}",
        error.message
    );
}
