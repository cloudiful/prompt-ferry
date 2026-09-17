use serde_json::{Value, json};

/// Stable identity for a reasoning item referenced by a Responses stream
/// event, preferring the explicit `item_id` over an embedded `item.id` and
/// falling back to the output index when neither is present.
pub(super) fn reasoning_key(value: &Value) -> String {
    value
        .get("item_id")
        .or_else(|| value.get("id"))
        .or_else(|| value.get("item").and_then(|item| item.get("id")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            value
                .get("output_index")
                .and_then(Value::as_u64)
                .map(|index| format!("output:{index}"))
                .unwrap_or_else(|| "output:0".to_string())
        })
}

pub(super) fn reasoning_item_added(
    output_index: Value,
    key: &str,
    token: Option<String>,
) -> Vec<u8> {
    let mut item = json!({
        "id": key,
        "type": "reasoning",
        "status": "in_progress",
        "summary": [],
        "content": [],
    });
    if let Some(token) = token {
        item["encrypted_content"] = Value::String(token);
    }
    sse_event(json!({
        "type": "response.output_item.added",
        "output_index": output_index,
        "item": item,
    }))
}

pub(super) fn summary_part_added(output_index: Value, key: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_part.added",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "part": {"type": "summary_text", "text": ""}
    }))
}

pub(super) fn summary_delta(output_index: Value, key: &str, delta: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_text.delta",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "delta": delta
    }))
}

pub(super) fn summary_part_done(output_index: Value, key: &str, text: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_part.done",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "part": {"type": "summary_text", "text": text}
    }))
}

pub(super) fn summary_text_done(output_index: Value, key: &str, text: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_text.done",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "text": text
    }))
}

fn sse_event(value: Value) -> Vec<u8> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    format!("event: {event_type}\ndata: {}\n\n", value).into_bytes()
}
