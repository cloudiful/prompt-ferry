use super::error_handling::{PassthroughSseFilter, ResponsesSseTerminal};
use serde_json::Value;

#[test]
fn responses_filter_requires_a_terminal_response_event() {
    let mut filter = PassthroughSseFilter::new_responses();
    filter
        .push_chunk(
            b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\"}\n\n",
        )
        .unwrap();
    filter.finish().unwrap();
    assert!(!filter.is_done());

    filter
        .push_chunk(b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n")
        .unwrap();
    assert!(filter.is_done());
    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Completed)
    );
}

#[test]
fn responses_filter_classifies_failure_terminals() {
    for (event_type, expected) in [
        ("response.failed", ResponsesSseTerminal::Failed),
        ("response.incomplete", ResponsesSseTerminal::Incomplete),
        ("error", ResponsesSseTerminal::Error),
    ] {
        let mut filter = PassthroughSseFilter::new_responses();
        let event = format!("data: {{\"type\":\"{event_type}\"}}\n\n");
        assert_eq!(filter.push_chunk(event.as_bytes()).unwrap().len(), 1);
        assert_eq!(filter.responses_terminal(), Some(expected));
    }
}

#[test]
fn responses_done_marker_is_a_success_terminal() {
    let mut filter = PassthroughSseFilter::new_responses();
    filter.push_chunk(b"data: [DONE]\r\n\r\n").unwrap();
    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Completed)
    );
}

#[test]
fn responses_filter_handles_split_crlf_terminal_and_drops_trailing_data() {
    let event = concat!(
        "event: response.completed\r\n",
        "data: {\"type\":\"response.completed\"}\r\n",
        "\r\n",
        "data: ignored\r\n\r\n"
    );
    let event_bytes = event.as_bytes();
    let split = event.find("completed").unwrap() + 3;
    let mut filter = PassthroughSseFilter::new_responses();

    assert!(filter.push_chunk(&event_bytes[..split]).unwrap().is_empty());
    let output = filter.push_chunk(&event_bytes[split..]).unwrap();
    assert_eq!(output.len(), 1);
    let trailing_start = event.find("data: ignored").unwrap();
    assert_eq!(output[0], &event_bytes[..trailing_start]);
    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Completed)
    );
    assert!(filter.finish().unwrap().is_empty());
}

#[test]
fn captures_responses_error_payload_for_diagnostics() {
    let mut filter = PassthroughSseFilter::new_responses();
    filter
        .push_chunk(
            b"event: error\r\ndata: {\"type\":\"error\",\"code\":\"upstream_busy\",\"message\":\"try again\"}\r\n\r\n",
        )
        .unwrap();

    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Error)
    );
    let body: Value = serde_json::from_str(filter.responses_error_body().unwrap()).unwrap();
    assert_eq!(body["code"], "upstream_busy");
    assert_eq!(body["message"], "try again");
}

#[test]
fn recognizes_error_event_without_a_type_field() {
    let mut filter = PassthroughSseFilter::new_responses();
    filter
        .push_chunk(b"event: error\ndata: provider failed\n\n")
        .unwrap();

    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Error)
    );
    assert_eq!(filter.responses_error_body(), Some("provider failed"));
}
