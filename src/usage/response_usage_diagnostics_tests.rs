use serde_json::{Map, json};

use super::response_usage_diagnostics::{
    MAX_USAGE_KEY_CHARS, MAX_USAGE_KEYS, TerminalEventCategory, UsageState, diagnose,
};
use crate::usage::{TokenUsage, UsageCapture};

#[test]
fn terminal_events_without_usage_report_absent_and_no_names() {
    for (event_type, category) in [
        ("response.completed", TerminalEventCategory::Completed),
        ("response.failed", TerminalEventCategory::Failed),
        ("response.incomplete", TerminalEventCategory::Incomplete),
    ] {
        let value = json!({
            "type": event_type,
            "response": { "id": "resp_1", "output": [] },
        });
        let diagnosis = diagnose(value.get("response").unwrap(), &value).unwrap();

        assert_eq!(diagnosis.category, category, "event={event_type}");
        assert_eq!(diagnosis.state, UsageState::Absent);
        assert!(diagnosis.usage_keys.is_empty());
    }
}

#[test]
fn terminal_events_with_unrecognized_usage_report_sanitized_key_names() {
    let long_key = "k".repeat(80);
    let mut usage = Map::new();
    usage.insert("completion_token_count".to_string(), json!(5));
    usage.insert("prompt_token_count".to_string(), json!("7"));
    usage.insert("nested".to_string(), json!({ "output_token_count": 3 }));
    usage.insert("odd name\nwith control".to_string(), json!(2));
    usage.insert(long_key, json!(1));
    let value = json!({
        "type": "response.completed",
        "response": { "usage": usage },
    });
    let diagnosis = diagnose(value.get("response").unwrap(), &value).unwrap();

    assert_eq!(diagnosis.category, TerminalEventCategory::Completed);
    assert_eq!(diagnosis.state, UsageState::Unrecognized);
    assert_eq!(
        diagnosis.usage_keys,
        vec![
            "completion_token_count".to_string(),
            format!("{}…", "k".repeat(MAX_USAGE_KEY_CHARS)),
            "nested".to_string(),
            "odd_name_with_control".to_string(),
            "prompt_token_count".to_string(),
        ]
    );
    assert!(
        !diagnosis
            .usage_keys
            .iter()
            .any(|key| key == "output_token_count"),
        "nested detail key names must not be reported"
    );
}

#[test]
fn caps_the_number_of_reported_usage_keys() {
    let mut usage = Map::new();
    for index in 0..MAX_USAGE_KEYS + 4 {
        usage.insert(format!("key_{index:02}"), json!(index));
    }
    let value = json!({
        "type": "response.incomplete",
        "response": { "usage": usage },
    });
    let diagnosis = diagnose(value.get("response").unwrap(), &value).unwrap();

    assert_eq!(diagnosis.usage_keys.len(), MAX_USAGE_KEYS);
    assert_eq!(diagnosis.usage_keys[0], "key_00".to_string());
}

#[test]
fn non_terminal_events_are_not_diagnosed() {
    let value = json!({ "type": "response.created", "response": {} });

    assert!(diagnose(value.get("response").unwrap(), &value).is_none());
}

#[test]
fn recognized_terminal_usage_parses_and_skips_diagnostics() {
    let event = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":120,\"output_tokens\":20,\"total_tokens\":140,\"input_tokens_details\":{\"cached_tokens\":30}}}}\n\n";
    let mut capture = UsageCapture::new(true, None);
    capture.observe_chunk(event);
    capture.finish();

    assert_eq!(capture.usage.input_tokens, Some(120));
    assert_eq!(capture.usage.output_tokens, Some(20));
    assert_eq!(capture.usage.total_tokens, Some(140));
    assert_eq!(capture.usage.cached_tokens, Some(30));

    let value = json!({
        "type": "response.completed",
        "response": {
            "usage": {
                "input_tokens": 120,
                "output_tokens": 20,
                "total_tokens": 140,
                "input_tokens_details": { "cached_tokens": 30 },
            }
        }
    });
    assert!(diagnose(value.get("response").unwrap(), &value).is_none());
}

#[test]
fn unrecognized_terminal_usage_stays_unknown_in_capture() {
    let event = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"token_usage\":{\"in\":1},\"id\":\"resp_1\"}}}\n\n";
    let mut capture = UsageCapture::new(true, None);
    capture.observe_chunk(event);
    capture.finish();

    assert_eq!(capture.usage, TokenUsage::default());
}
