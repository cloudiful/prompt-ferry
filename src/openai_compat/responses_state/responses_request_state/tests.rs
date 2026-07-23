#![cfg(test)]

use std::collections::HashSet;

use super::{
    NormalizedResponsesRequest, normalize_response_error, raw_responses_input_items_from_body,
    validate_raw_responses_request_body,
};

#[test]
fn lifts_leading_system_messages_into_instructions() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"role":"system","content":"be terse"},{"role":"user","content":"hi"}]}"#,
    )
    .unwrap();
    assert_eq!(request.instructions.as_deref(), Some("be terse"));
    assert_eq!(request.items[0]["role"].as_str(), Some("user"));
}

#[test]
fn lifts_leading_developer_messages_into_instructions() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"instructions":"base","input":[{"role":"developer","content":"use tools carefully"},{"role":"user","content":"hi"}]}"#,
    )
    .unwrap();
    assert_eq!(
        request.instructions.as_deref(),
        Some("base\n\nuse tools carefully")
    );
    assert_eq!(request.items[0]["role"].as_str(), Some("user"));
}

#[test]
fn preserves_leading_additional_tools_items() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"role":"developer","type":"additional_tools","tools":[{"name":"echo","type":"function","parameters":{"type":"object"}}]},{"role":"user","content":"hi"}]}"#,
    )
    .unwrap();
    assert_eq!(request.instructions, None);
    assert_eq!(request.items[0]["type"].as_str(), Some("additional_tools"));
    assert_eq!(request.items[0]["role"].as_str(), Some("developer"));
    assert_eq!(request.items[1]["role"].as_str(), Some("user"));
}

#[test]
fn validates_raw_passthrough_with_leading_additional_tools_items() {
    let body = br#"{"input":[{"role":"developer","type":"additional_tools","tools":[{"name":"echo","type":"function","parameters":{"type":"object"}}]},{"role":"user","content":"hi"}]}"#;
    validate_raw_responses_request_body(body).unwrap();
    let items = raw_responses_input_items_from_body(body).unwrap();
    assert_eq!(items[0]["type"].as_str(), Some("additional_tools"));
    assert_eq!(items[0]["role"].as_str(), Some("developer"));
}

#[test]
fn validates_same_request_function_call_output() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"},{"type":"function_call_output","call_id":"call_1","output":"ok"}]}"#,
    )
    .unwrap();
    request
        .validate_for_chat_compat(&HashSet::new(), false)
        .unwrap();
}

#[test]
fn folds_non_leading_instruction_messages_into_chat_compat_instructions() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"instructions":"base","input":[{"role":"user","content":"hi"},{"role":"developer","content":"use tools carefully"},{"role":"assistant","content":"ok"},{"role":"system","content":"stay terse"}]}"#,
    )
    .unwrap();
    request
        .validate_for_chat_compat(&HashSet::new(), false)
        .unwrap();
    assert_eq!(
        request.chat_compat_instructions().unwrap().as_deref(),
        Some("base\n\nuse tools carefully\n\nstay terse")
    );
}

#[test]
fn normalizes_assistant_messages_for_responses_native_upstream() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"role":"assistant","content":[{"type":"input_text","text":"prior answer"}]},{"role":"user","content":"next question"}]}"#,
    )
    .unwrap();
    let encoded = request
        .to_responses_request_with_prefix(&[], false, false)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        body["input"][0]["content"][0]["type"].as_str(),
        Some("output_text")
    );
    assert_eq!(
        body["input"][0]["content"][0]["text"].as_str(),
        Some("prior answer")
    );
}

#[test]
fn forces_store_true_for_responses_native_tool_requests() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"model":"gpt-test","tools":[{"type":"function","name":"get_current_time","description":"Get time","parameters":{"type":"object","properties":{}}}],"input":[{"role":"user","content":"what time is it?"}]}"#,
    )
    .unwrap();
    let encoded = request
        .to_responses_request_with_prefix(&[], false, false)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(body["store"].as_bool(), Some(true));
}

#[test]
fn rejects_unpaired_function_call_output() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"type":"function_call_output","call_id":"call_1","output":"ok"}]}"#,
    )
    .unwrap();
    let err = request
        .validate_for_chat_compat(&HashSet::new(), false)
        .unwrap_err();
    assert_eq!(err.code, "invalid_responses_continuation");
}

#[test]
fn rejects_unresolved_item_reference_without_replay_context() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"type":"item_reference","id":"call_1"},{"role":"user","content":"use it"}]}"#,
    )
    .unwrap();
    let err = request
        .validate_for_chat_compat(&HashSet::new(), false)
        .unwrap_err();
    assert_eq!(err.code, "invalid_responses_continuation");
}

#[test]
fn strips_item_references_from_chat_compat_messages() {
    let request = NormalizedResponsesRequest::from_body(
        br#"{"input":[{"type":"item_reference","id":"call_1"},{"type":"function_call_output","call_id":"call_1","output":"72F"}]}"#,
    )
    .unwrap();
    let prior_call_ids = HashSet::from([String::from("call_1")]);
    request
        .validate_for_chat_compat(&prior_call_ids, true)
        .unwrap();
    let items = request.chat_compat_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["type"].as_str(), Some("function_call_output"));
}

#[test]
fn wraps_detail_errors_in_openai_shape() {
    let value =
        normalize_response_error(r#"{"detail":"Unsupported parameter: previous_response_id"}"#);
    assert_eq!(
        value["error"]["message"].as_str(),
        Some("Unsupported parameter: previous_response_id")
    );
}
