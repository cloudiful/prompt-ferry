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
    let parents = HashMap::from([(0usize, 101), (2usize, 102)]);
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
fn restores_all_tool_calls_from_the_same_assistant_turn() {
    let mut messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[
            {"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}},
            {"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}
        ]
    })];
    let parents = HashMap::from([(0usize, 101)]);
    let artifacts = HashMap::from([(
        101,
        json!({
            "role":"assistant",
            "content":null,
            "reasoning_content":"reasoning for both calls",
            "tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}},
                {"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}
            ]
        }),
    )]);

    restore_reasoning_from_replay(&mut messages, &parents, &artifacts).unwrap();

    assert_eq!(messages[0]["reasoning_content"], "reasoning for both calls");
}

#[test]
fn rejects_a_tool_call_message_that_mixes_parent_events() {
    let messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[
            {"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}},
            {"id":"call_2","type":"function","function":{"name":"two","arguments":"{}"}}
        ]
    })];
    let candidates = HashMap::from([
        ("call_1".to_string(), vec![(101, true)]),
        ("call_2".to_string(), vec![(102, true)]),
    ]);
    let error = resolve_replay_parents(&messages, &candidates, &HashMap::new(), true).unwrap_err();

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
    let parents = HashMap::from([(0usize, 101)]);
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
fn classifies_a_leftover_tool_call_without_an_artifact() {
    let messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
    })];
    let candidates = HashMap::from([("call_1".to_string(), vec![(101, false)])]);

    let error = resolve_replay_parents(&messages, &candidates, &HashMap::new(), true).unwrap_err();

    assert!(error.message.contains("missing_artifact"));
}

#[test]
fn rejects_duplicate_call_id_parents_without_provenance() {
    let messages = vec![json!({
        "role":"assistant",
        "content":null,
        "tool_calls":[{"id":"call_1","type":"function","function":{"name":"one","arguments":"{}"}}]
    })];
    let candidates = HashMap::from([("call_1".to_string(), vec![(101, true), (102, true)])]);

    let error = resolve_replay_parents(&messages, &candidates, &HashMap::new(), false).unwrap_err();

    assert!(error.message.contains("ambiguous_parent"));
}

#[test]
fn resolves_reused_call_id_per_assistant_turn_with_provenance() {
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
            "tool_calls":[{"id":"call_1","type":"function","function":{"name":"two","arguments":"{\"value\":2}"}}]
        }),
    ];
    let candidates = HashMap::from([("call_1".to_string(), vec![(101, true), (102, true)])]);
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
                "tool_calls":[{"id":"call_1","type":"function","function":{"name":"two","arguments":"{ \"value\": 2 }"}}]
            }),
        ),
    ]);

    let parents = resolve_replay_parents(&messages, &candidates, &artifacts, true).unwrap();
    restore_reasoning_from_replay(&mut messages, &parents, &artifacts).unwrap();

    assert_eq!(parents, HashMap::from([(0usize, 101), (2usize, 102)]));
    assert_eq!(messages[0]["reasoning_content"], "reasoning one");
    assert_eq!(messages[2]["reasoning_content"], "reasoning two");
}

#[tokio::test]
async fn bypasses_reasoning_recovery_for_non_reasoning_providers() {
    let route = db::RouteConfig {
        route_id: uuid::Uuid::nil(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.openai.example".to_string(),
        api_key: "key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: crate::config::NativeApi::Chat,
        upstream_model: None,
        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
    };
    let body = serde_json::to_vec(&json!({
        "model":"gpt-test",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(None, Some(1), &route, None, None, &body)
        .await
        .unwrap();

    assert!(restored.is_none());
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
