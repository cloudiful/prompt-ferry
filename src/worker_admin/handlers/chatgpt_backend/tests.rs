use std::borrow::Cow;

use serde_json::{Value, json};

use super::normalize_codex_request_body;

fn normalize(body: &Value) -> Value {
    let bytes = serde_json::to_vec(body).expect("test body serializes");
    let normalized = normalize_codex_request_body(&bytes);
    serde_json::from_slice(&normalized).expect("normalized body stays JSON")
}

#[test]
fn drops_max_output_tokens_for_the_subscription_backend() {
    let value = normalize(&json!({
        "model": "gpt-5.6-luna",
        "input": [{"type": "message", "role": "user", "content": "hi"}],
        "store": true,
        "max_output_tokens": 64000,
    }));
    assert!(value.get("max_output_tokens").is_none());
    assert_eq!(value["model"], json!("gpt-5.6-luna"));
    assert_eq!(value["store"], json!(false));
    assert!(value.get("input").is_some());
}

#[test]
fn keeps_the_parameters_the_subscription_backend_accepts() {
    let body = json!({
        "model": "gpt-5.6-luna",
        "instructions": "be concise",
        "input": [],
        "tools": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": {"effort": "high", "summary": "auto"},
        "stream": true,
        "stream_options": {"include_usage": true},
        "include": ["reasoning.encrypted_content"],
        "service_tier": "priority",
        "prompt_cache_key": "cache-1",
        "text": {"verbosity": "low"},
        "client_metadata": {"x-codex-window-id": "thread_1:0"},
        "access_programs": ["chatgpt_plus"],
        "max_output_tokens": 1024,
    });
    let value = normalize(&body);
    // Locks the accepted Codex request shape listed on
    // `CODEX_UNSUPPORTED_PARAMS`; keep the two in sync.
    for field in [
        "instructions",
        "input",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "reasoning",
        "stream",
        "stream_options",
        "include",
        "service_tier",
        "prompt_cache_key",
        "text",
        "client_metadata",
        "access_programs",
    ] {
        assert_eq!(value[field], body[field], "field {field} changed");
    }
    assert!(value.get("max_output_tokens").is_none());
}

#[test]
fn drops_max_output_tokens_from_an_otherwise_stable_body() {
    // Model and `store` already match the Codex shape, so removing the
    // unsupported parameter is the only rewrite and the body must come back
    // owned instead of borrowed.
    let bytes = br#"{"model":"gpt-5.6-luna","input":[],"store":false,"max_output_tokens":4096}"#;
    let normalized = normalize_codex_request_body(bytes);
    assert!(matches!(normalized, Cow::Owned(_)));
    let value: Value =
        serde_json::from_slice(normalized.as_ref()).expect("normalized body stays JSON");
    assert!(value.get("max_output_tokens").is_none());
    assert_eq!(value["store"], json!(false));
    assert_eq!(value["input"], json!([]));
}

#[test]
fn borrows_a_body_that_needs_no_rewrite() {
    let bytes = br#"{"model":"gpt-5.6-luna","input":[],"store":false}"#;
    assert!(matches!(
        normalize_codex_request_body(bytes),
        Cow::Borrowed(_)
    ));
}

#[test]
fn passes_non_json_and_non_object_bodies_through_unchanged() {
    for body in [&b"not json"[..], &b"[1,2,3]"[..], &b"null"[..]] {
        assert!(matches!(
            normalize_codex_request_body(body),
            Cow::Borrowed(_)
        ));
    }
}
