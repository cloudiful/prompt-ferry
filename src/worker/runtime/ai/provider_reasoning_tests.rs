use super::*;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn restores_reasoning_for_tool_calls_from_multiple_parent_events() {
    let mut messages = vec![
        json!({
            "role":"assistant",
            "content":null,
            "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
        }),
        json!({"role":"tool","tool_call_id":"call_1","content":"one"}),
        json!({
            "role":"assistant",
            "content":null,
            "tool_calls":[{"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}]
        }),
        json!({"role":"tool","tool_call_id":"call_2","content":"two"}),
    ];
    let parents = HashMap::from([("call_1".to_string(), 101), ("call_2".to_string(), 102)]);
    let artifacts = HashMap::from([
        (
            101,
            json!({
                "role":"assistant",
                "content":null,
                "reasoning_content":"reasoning one",
                "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
            }),
        ),
        (
            102,
            json!({
                "role":"assistant",
                "content":null,
                "reasoning_content":"reasoning two",
                "tool_calls":[{"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}]
            }),
        ),
    ]);

    restore_reasoning_from_replay(&mut messages, &parents, &artifacts).unwrap();

    assert_eq!(messages[0]["reasoning_content"], "reasoning one");
    assert_eq!(messages[2]["reasoning_content"], "reasoning two");
}

#[test]
fn rejects_a_tool_call_message_that_mixes_parent_events() {
    let mut messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[
            {"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}},
            {"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}
        ]
    })];
    let parents = HashMap::from([("call_1".to_string(), 101), ("call_2".to_string(), 102)]);
    let artifacts = HashMap::new();

    let error = restore_reasoning_from_replay(&mut messages, &parents, &artifacts).unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("mixes replay parents"));
}

#[test]
fn rejects_missing_reasoning_before_forwarding() {
    let mut messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
    })];
    let parents = HashMap::from([("call_1".to_string(), 101)]);
    let artifacts = HashMap::from([(
        101,
        json!({
            "role":"assistant",
            "content":null,
            "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
        }),
    )]);

    let error = restore_reasoning_from_replay(&mut messages, &parents, &artifacts).unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing complete reasoning"));
}
