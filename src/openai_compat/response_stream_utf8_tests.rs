use super::response_stream::{AnthropicResponseStreamAdapter, ChatResponseStreamAdapter};
use serde_json::{Value, json};

#[test]
fn preserves_split_utf8_in_chat_to_responses_stream() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let bytes = chat_stream("近期催化");
    let (first, second) = split_inside(bytes.as_slice(), "近期催化");

    let mut output = adapter.push_chunk(first).unwrap();
    output.extend(adapter.push_chunk(second).unwrap());

    let text = String::from_utf8(output.concat()).unwrap();
    assert!(!text.contains('\u{fffd}'));
    assert_eq!(completed_output_text(&text), "近期催化");
}

#[test]
fn preserves_split_utf8_in_anthropic_to_responses_stream() {
    let mut adapter = AnthropicResponseStreamAdapter::new();
    let bytes = anthropic_stream("近期催化");
    let (first, second) = split_inside(bytes.as_slice(), "近期催化");

    let mut output = adapter.push_chunk(first).unwrap();
    output.extend(adapter.push_chunk(second).unwrap());

    let text = String::from_utf8(output.concat()).unwrap();
    assert!(!text.contains('\u{fffd}'));
    assert_eq!(completed_output_text(&text), "近期催化");
}

fn chat_stream(content: &str) -> Vec<u8> {
    let event = json!({
        "id": "chatcmpl_test",
        "created": 123,
        "model": "deepseek-test",
        "choices": [{"delta": {"content": content}}]
    });
    format!("data: {event}\n\ndata: [DONE]\n\n").into_bytes()
}

fn anthropic_stream(content: &str) -> Vec<u8> {
    let start = json!({
        "type": "message_start",
        "message": {"id": "msg_test", "model": "claude-test"}
    });
    let block_start = json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": {"type": "text", "text": ""}
    });
    let delta = json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": {"type": "text_delta", "text": content}
    });
    let stop = json!({"type": "message_stop"});
    format!("data: {start}\n\ndata: {block_start}\n\ndata: {delta}\n\ndata: {stop}\n\n")
        .into_bytes()
}

fn split_inside<'a>(bytes: &'a [u8], needle: &str) -> (&'a [u8], &'a [u8]) {
    let start = bytes
        .windows(needle.len())
        .position(|window| window == needle.as_bytes())
        .expect("test stream contains the target text");
    let split = start + 1;
    (&bytes[..split], &bytes[split..])
}

fn completed_output_text(text: &str) -> String {
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .and_then(|event| event["response"]["output_text"].as_str().map(str::to_owned))
        .expect("stream contains a completed response")
}
