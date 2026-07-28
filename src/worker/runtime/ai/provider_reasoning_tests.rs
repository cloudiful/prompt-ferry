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

#[test]
fn matches_tool_calls_when_json_argument_formatting_differs() {
    let current = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "lookup",
                "arguments": "{\"city\":\"Boston\",\"options\":{\"limit\":5,\"active\":true}}"
            }
        }]
    });
    let artifact = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "lookup",
                "arguments": "{ \"options\": { \"active\": true, \"limit\": 5 }, \"city\": \"Boston\" }"
            }
        }]
    });

    assert!(tool_calls_match(&current, &artifact));
}

#[test]
fn rejects_tool_calls_when_json_argument_value_differs() {
    let current = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {"name": "lookup", "arguments": "{\"limit\":5}"}
        }]
    });
    let artifact = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {"name": "lookup", "arguments": "{\"limit\":6}"}
        }]
    });

    assert!(!tool_calls_match(&current, &artifact));
}

#[test]
fn compares_invalid_json_arguments_as_raw_strings() {
    let current = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {"name": "lookup", "arguments": "not-json"}
        }]
    });
    let same_artifact = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {"name": "lookup", "arguments": "not-json"}
        }]
    });
    let different_artifact = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {"name": "lookup", "arguments": "still-not-json"}
        }]
    });

    assert!(tool_calls_match(&current, &same_artifact));
    assert!(!tool_calls_match(&current, &different_artifact));
}
