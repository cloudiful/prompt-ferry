use serde_json::json;

#[test]
fn orphan_tool_message_keeps_call_id_in_responses_input() {
    let body = json!({
        "model": "test",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "tool", "tool_call_id": "call_orphan_1", "content": "out"}
        ]
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    let out = prompt_ferry::openai_compat::chat_request_to_responses(&bytes)
        .expect("orphan output must stay translatable");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("call_orphan_1"),
        "call_id must survive, got {text}"
    );
}

#[test]
fn missing_tool_output_is_synthesized_with_call_id() {
    let body = json!({
        "model": "test",
        "messages": [
            {"role": "assistant", "content": null,
             "tool_calls": [{"id": "call_missing_1", "type": "function",
                "function": {"name": "bash", "arguments": "{}"}}]}
        ]
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    let out = prompt_ferry::openai_compat::chat_request_to_responses(&bytes)
        .expect("missing output must synthesize placeholder");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("call_missing_1"),
        "call_id must survive, got {text}"
    );
    assert!(
        text.contains("Missing tool output"),
        "placeholder must be explicit, got {text}"
    );
}

#[test]
fn upstream_reasoning_expired_maps_to_retryable() {
    let mapped = prompt_ferry::map_upstream_invalid_request(
        "Referenced reasoning item 'rs_abc:rs_xyz' was not found or has expired",
    );
    assert!(mapped.is_some());
    let m = mapped.unwrap();
    assert_eq!(m.code, "retryable_invalid_continuation");
    assert!(m.hint.contains("previous_response_id"));
}
