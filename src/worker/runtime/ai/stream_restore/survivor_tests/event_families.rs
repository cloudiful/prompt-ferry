//! Event-family restore arms and SSE framing assertions.

use serde_json::Value;

use super::{SseRestoreFilter, data_json, session};

#[test]
fn restores_function_call_arguments_done_event() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = format!(
        "data: {{\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc1\",\"output_index\":0,\"arguments\":{token:?}}}\n\n"
    );

    let output = filter.push_chunk(event.as_bytes()).expect("restore");
    filter.finish().expect("finish");

    assert_eq!(data_json(&output[0])["arguments"], "a.example.com");
}

#[test]
fn restores_anthropic_content_block_start_text() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let event = format!(
        "data: {{\"type\":\"content_block_start\",\"index\":2,\"content_block\":{{\"type\":\"text\",\"text\":{token:?}}}}}\n\n"
    );

    let output = filter.push_chunk(event.as_bytes()).expect("restore");
    filter.finish().expect("finish");

    let value = data_json(&output[0]);
    assert_eq!(value["content_block"]["text"], "a.example.com");
    assert_eq!(value["index"], 2);
    // No delta pointer exists on a start event; the delta branch must not run.
    assert!(value.get("delta").is_none());
}

#[test]
fn restores_anthropic_content_block_delta_text() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let event = format!(
        "data: {{\"type\":\"content_block_delta\",\"index\":3,\"delta\":{{\"type\":\"text_delta\",\"text\":{token:?}}}}}\n\n"
    );

    let output = filter.push_chunk(event.as_bytes()).expect("restore");
    filter.finish().expect("finish");

    let value = data_json(&output[0]);
    assert_eq!(value["delta"]["text"], "a.example.com");
    assert_eq!(value["index"], 3);
    assert!(value.get("content_block").is_none());
}

#[test]
fn restores_anthropic_delta_split_across_two_events_with_distinct_indexes() {
    let (session, token) = session("a.example.com");
    let split = token.len() / 2;
    let mut filter = SseRestoreFilter::new(&session);
    let first = format!(
        "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"text\":{:?}}}}}\n\n",
        &token[..split]
    );
    let second = format!(
        "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"text\":{:?}}}}}\n\n",
        &token[split..]
    );

    let first = filter.push_chunk(first.as_bytes()).expect("first");
    let second = filter.push_chunk(second.as_bytes()).expect("second");
    filter.finish().expect("finish");

    assert_eq!(data_json(&first[0])["delta"]["text"], "");
    assert_eq!(data_json(&second[0])["delta"]["text"], "a.example.com");
}

#[test]
fn restored_event_keeps_the_event_line_prefix_byte_exact() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let event = format!(
        "event: response.output_text.delta\r\ndata: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg\",\"output_index\":0,\"content_index\":0,\"delta\":{token:?}}}\r\n\r\n"
    );

    let output = filter.push_chunk(event.as_bytes()).expect("restore");

    let text = std::str::from_utf8(&output[0]).expect("UTF-8");
    assert!(
        text.starts_with("event: response.output_text.delta\r\n"),
        "the event: line must survive the reframe: {text:?}"
    );
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    let value: Value = serde_json::from_str(payload).expect("JSON");
    assert_eq!(value["delta"], "a.example.com");
}

#[test]
fn multi_line_sse_data_is_rejected() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let err = filter
        .push_chunk(b"data: first\ndata: second\n\n")
        .expect_err("multi-line data");
    assert!(err.to_string().contains("multi-line SSE data"));
}
