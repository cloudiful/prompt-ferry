//! Mutation-survivor assertions for the pending-stream flush helpers (#569
//! Phase 2): the synthetic delta framing, the blanking of already-seen stream
//! text, and the pointer guard behavior.

use serde_json::{Value, json};

use super::{blank_stream_text, set_empty, synthetic_delta};

#[test]
fn synthetic_delta_frames_response_events_with_an_event_line() {
    let template = json!({
        "type": "response.output_text.delta",
        "item_id": "msg",
        "delta": "[[RDX:v2:text:domain:001:cafebabe]]"
    });

    let output = synthetic_delta(template, "/delta", "a.example.com".to_string()).expect("flush");

    let text = std::str::from_utf8(&output).expect("UTF-8");
    assert!(
        text.starts_with("event: response.output_text.delta\n"),
        "responses family must carry an event line: {text:?}"
    );
    assert!(text.ends_with("\n\n"));
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    let value: Value = serde_json::from_str(payload).expect("JSON");
    assert_eq!(value["delta"], "a.example.com");
}

#[test]
fn synthetic_delta_frames_non_response_events_as_plain_data() {
    let template = json!({
        "type": "content_block_delta",
        "index": 1,
        "delta": {"text": "[[RDX:v2:text:domain:001:cafebabe]]"}
    });

    let output =
        synthetic_delta(template, "/delta/text", "a.example.com".to_string()).expect("flush");

    let text = std::str::from_utf8(&output).expect("UTF-8");
    assert!(
        text.starts_with("data: {"),
        "non-responses family must not carry an event line: {text:?}"
    );
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    let value: Value = serde_json::from_str(payload).expect("JSON");
    assert_eq!(value["delta"]["text"], "a.example.com");
}

#[test]
fn synthetic_delta_without_the_stream_pointer_is_an_error() {
    let template = json!({"type": "response.output_text.delta", "item_id": "msg"});
    let err =
        synthetic_delta(template, "/delta", "orphan".to_string()).expect_err("missing pointer");
    assert!(err.to_string().contains("missing pending stream field"));
}

#[test]
fn blank_stream_text_empties_responses_delta_pointers() {
    let mut value = json!({
        "type": "response.output_text.delta",
        "item_id": "msg",
        "delta": "already streamed"
    });
    blank_stream_text(&mut value);
    assert_eq!(
        value["delta"], "",
        "delta must be blanked before reinsertion"
    );

    let mut reasoning = json!({
        "type": "response.reasoning_text.delta",
        "delta": "part"
    });
    blank_stream_text(&mut reasoning);
    assert_eq!(reasoning["delta"], "");

    let mut arguments = json!({
        "type": "response.function_call_arguments.delta",
        "delta": "{\"x\":"
    });
    blank_stream_text(&mut arguments);
    assert_eq!(arguments["delta"], "");
}

#[test]
fn blank_stream_text_empties_anthropic_start_and_delta_pointers() {
    let mut start = json!({
        "type": "content_block_start",
        "content_block": {"text": "seen", "thinking": "thought", "partial_json": "{}"}
    });
    blank_stream_text(&mut start);
    assert_eq!(start["content_block"]["text"], "");
    assert_eq!(start["content_block"]["thinking"], "");
    assert_eq!(start["content_block"]["partial_json"], "");

    let mut delta = json!({
        "type": "content_block_delta",
        "delta": {"text": "seen", "thinking": "thought", "partial_json": "{}"}
    });
    blank_stream_text(&mut delta);
    assert_eq!(delta["delta"]["text"], "");
    assert_eq!(delta["delta"]["thinking"], "");
    assert_eq!(delta["delta"]["partial_json"], "");
}

#[test]
fn blank_stream_text_empties_chat_choice_stream_fields() {
    let mut value = json!({
        "choices": [{
            "index": 0,
            "delta": {
                "content": "seen",
                "reasoning_content": "thought",
                "reasoning": "r",
                "refusal": "no",
                "reasoning_details": [{"text": "detail"}],
                "tool_calls": [{"index": 0, "function": {"arguments": "{\"a\""}}]
            }
        }]
    });
    blank_stream_text(&mut value);
    let delta = &value["choices"][0]["delta"];
    assert_eq!(delta["content"], "");
    assert_eq!(delta["reasoning_content"], "");
    assert_eq!(delta["reasoning"], "");
    assert_eq!(delta["refusal"], "");
    assert_eq!(delta["reasoning_details"][0]["text"], "");
    assert_eq!(delta["tool_calls"][0]["function"]["arguments"], "");
}

#[test]
fn blank_stream_text_leaves_missing_and_non_string_fields_alone() {
    let mut value = json!({
        "type": "response.output_text.delta",
        "item_id": "msg",
        "delta": 7
    });
    blank_stream_text(&mut value);
    assert_eq!(value["delta"], 7, "non-string field must be untouched");

    let mut absent = json!({"type": "response.output_text.delta", "item_id": "msg"});
    blank_stream_text(&mut absent);
    assert!(absent.get("delta").is_none(), "absent field stays absent");
}

#[test]
fn set_empty_only_rewrites_string_targets() {
    let mut value = json!({"path": "seen", "number": 3, "nested": {"leaf": "x"}});
    set_empty(&mut value, "/path");
    assert_eq!(value["path"], "");
    set_empty(&mut value, "/number");
    assert_eq!(value["number"], 3, "non-string target must be untouched");
    set_empty(&mut value, "/nested/leaf");
    assert_eq!(value["nested"]["leaf"], "");
    set_empty(&mut value, "/missing");
    assert!(value.get("missing").is_none());
}
