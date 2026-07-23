use serde_json::{Value, json};

use crate::openai_compat::{assistant_message_to_output_items, persisted_artifact};
use crate::stream_text::Utf8LineDecoder;

use super::AssistantArtifact;

pub(super) const MAX_JSON_CAPTURE: usize = 1024 * 1024;

pub fn fallback_text_artifact(text: &str) -> Option<AssistantArtifact> {
    let content = text.trim();
    if content.is_empty() {
        return None;
    }
    let assistant_message = json!({
        "role": "assistant",
        "content": content,
    });
    let output_items = assistant_message_to_output_items(&assistant_message).ok()?;
    let (message_json, has_reasoning_content, has_tool_calls) =
        persisted_artifact(Some(assistant_message), output_items)?;
    Some(AssistantArtifact {
        message_json,
        has_reasoning_content,
        has_tool_calls,
    })
}

pub(super) fn finish_json_capture(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<Value>(bytes).ok()
}

pub(super) fn observe_json_chunk(
    json_body: &mut Vec<u8>,
    json_body_truncated: &mut bool,
    chunk: &[u8],
) {
    if *json_body_truncated {
        return;
    }
    if json_body.len().saturating_add(chunk.len()) <= MAX_JSON_CAPTURE {
        json_body.extend_from_slice(chunk);
    } else {
        json_body.clear();
        *json_body_truncated = true;
    }
}

pub(super) fn finish_sse_line(decoder: &mut Utf8LineDecoder) -> Option<String> {
    decoder.finish().ok().flatten()
}

pub(super) fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(extract_text).collect::<Vec<_>>().join(""),
        Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("output_text"))
            .map(extract_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(super) fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}
