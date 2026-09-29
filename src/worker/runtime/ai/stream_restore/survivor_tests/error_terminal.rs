//! Responses error-body capture assertions.

use serde_json::{Value, json};

use super::{SseRestoreFilter, delta_event, session};
use crate::worker::runtime::error_handling::ResponsesSseTerminal;

#[test]
fn responses_error_body_is_none_until_a_terminal_error_arrives() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    assert_eq!(filter.responses_error_body(), None, "no error yet");

    filter
        .push_chunk(delta_event("msg", 0, "plain").as_bytes())
        .expect("delta");
    assert_eq!(filter.responses_error_body(), None);

    let failure = json!({
        "type": "response.failed",
        "response": {"error": {"code": "server_error", "message": "upstream exploded"}}
    });
    filter
        .push_chunk(format!("data: {failure}\n\n").as_bytes())
        .expect("failure terminal");

    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Failed)
    );
    let body = filter.responses_error_body().expect("error body captured");
    assert!(!body.is_empty(), "captured body is never empty");
    let parsed: Value = serde_json::from_str(body).expect("body is JSON");
    assert_eq!(parsed["type"], "response.failed");
    assert_eq!(parsed["response"]["error"]["code"], "server_error");
}

#[test]
fn responses_error_event_body_is_the_pretty_json_of_the_payload() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let error = json!({"type": "error", "code": "upstream_busy", "message": "try again"});
    filter
        .push_chunk(format!("data: {error}\n\n").as_bytes())
        .expect("error terminal");

    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Error)
    );
    let body = filter.responses_error_body().expect("error body captured");
    let parsed: Value = serde_json::from_str(body).expect("body is JSON");
    assert_eq!(parsed, error);
}

#[test]
fn named_error_event_without_type_field_still_captures_a_body() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    filter
        .push_chunk(b"event: error\ndata: {\"message\":\"provider failed\"}\n\n")
        .expect("named error event");

    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Error)
    );
    let body = filter.responses_error_body().expect("error body captured");
    let parsed: Value = serde_json::from_str(body).expect("body is JSON");
    assert_eq!(parsed["message"], "provider failed");
}
