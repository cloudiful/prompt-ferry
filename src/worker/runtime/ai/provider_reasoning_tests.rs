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
    let route = sample_route(
        "https://api.openai.example",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"gpt-test",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap();

    assert!(restored.is_none());
}

#[tokio::test]
async fn skips_deepseek_reasoning_recovery_for_chat_to_responses() {
    let route = sample_route(
        "https://api.deepseek.com",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-v4-flash",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::ChatToResponses,
        &body,
    )
    .await
    .unwrap();

    assert!(restored.is_none());
}

#[tokio::test]
async fn auto_skips_proxy_endpoint_with_deepseek_model_prefix() {
    let route = sample_route(
        "https://gateway.example.com",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-v4-flash",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap();

    assert!(restored.is_none());
}

#[tokio::test]
async fn auto_triggers_for_direct_deepseek_endpoint_and_reasoning_model() {
    let route = sample_route(
        "https://api.deepseek.com",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-v4-flash",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let error = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing_artifact"));
    assert!(error.message.contains("before forwarding"));
}

#[tokio::test]
async fn auto_triggers_for_direct_minimax_endpoint_and_reasoning_model() {
    let route = sample_route(
        "https://api.minimax.io",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"MiniMax-M3",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let error = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing_artifact"));
}

#[tokio::test]
async fn responses_auto_triggers_for_direct_minimax_endpoint_and_reasoning_model() {
    let route = sample_route(
        "https://api.minimax.io",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"MiniMax-M3",
        "input":[{
            "type":"function_call",
            "call_id":"call_1",
            "name":"one",
            "arguments":"{}"
        }]
    }))
    .unwrap();

    let error =
        super::responses::restore_responses_reasoning(None, Some(1), &route, None, None, &body)
            .await
            .unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing_artifact"));
}

#[tokio::test]
async fn auto_skips_direct_deepseek_endpoint_with_non_reasoning_model() {
    let route = sample_route(
        "https://api.deepseek.com",
        db::ChatReasoningReplayPolicy::Auto,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-chat",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap();

    assert!(restored.is_none());
}

#[tokio::test]
async fn force_replay_overrides_auto_judgment_for_proxy_endpoint() {
    let route = sample_route(
        "https://gateway.example.com",
        db::ChatReasoningReplayPolicy::ForceReplay,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-v4-flash",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let error = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "replay_unavailable");
    assert!(error.message.contains("missing_artifact"));
}

#[tokio::test]
async fn force_passthrough_overrides_model_prefix_judgment() {
    let route = sample_route(
        "https://api.deepseek.com",
        db::ChatReasoningReplayPolicy::ForcePassthrough,
    );
    let body = serde_json::to_vec(&json!({
        "model":"deepseek-v4-flash",
        "messages":[{
            "role":"assistant",
            "tool_calls":[{"id":"call_1","function":{"name":"one","arguments":"{}"}}]
        }]
    }))
    .unwrap();

    let restored = restore_provider_reasoning(
        None,
        Some(1),
        &route,
        None,
        None,
        crate::upstream_adapter::ResponseAdapter::Passthrough,
        &body,
    )
    .await
    .unwrap();

    assert!(restored.is_none());
}

fn sample_route(
    base_url: &str,
    chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy,
) -> db::RouteConfig {
    db::RouteConfig {
        route_id: uuid::Uuid::nil(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: base_url.to_string(),
        api_key: "key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: crate::config::NativeApi::Chat,
        upstream_model: None,
        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
        chat_reasoning_replay_policy,
        route_selection_reason: db::RouteSelectionReason::Default,
    }
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
fn matches_tool_calls_when_capture_repaired_invalid_arguments() {
    let current = json!({
        "content":"<tool_call>\n<function=search_stocks>\n<parameter=query>正泰电源</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>",
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "search_stocks",
                "arguments": "{\"query\": "
            }
        }]
    });
    let artifact = json!({
        "content":"<tool_call>\n<function=search_stocks>\n<parameter=query>正泰电源</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>",
        "reasoning_content":"reasoning",
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "search_stocks",
                "arguments": "{\"limit\":5,\"query\":\"正泰电源\"}"
            }
        }]
    });

    assert!(tool_calls_match(&current, &artifact));
}

#[test]
fn rejects_unrepairable_invalid_arguments_against_valid_artifact() {
    let current = json!({
        "content":"plain text",
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "search_stocks",
                "arguments": "{\"query\": "
            }
        }]
    });
    let artifact = json!({
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "search_stocks",
                "arguments": "{\"query\":\"正泰电源\"}"
            }
        }]
    });

    assert!(!tool_calls_match(&current, &artifact));
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
