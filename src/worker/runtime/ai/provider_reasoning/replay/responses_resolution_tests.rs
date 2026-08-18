use super::{ResponsesReplayToolCall, resolve_responses_replay_groups};
use serde_json::json;
use std::collections::HashMap;

fn call(input_index: usize, call_id: &str, name: &str) -> ResponsesReplayToolCall {
    ResponsesReplayToolCall {
        input_index,
        call_id: call_id.to_string(),
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

fn assert_replay_failure(error: &crate::openai_compat::CompatError, kind: &str) {
    assert_eq!(error.code, "replay_unavailable");
    assert!(
        error.message.starts_with(kind),
        "expected {kind} replay failure, got {}",
        error.message
    );
}
