use serde_json::{Value, json};
use tracing::warn;

use crate::openai_compat::{assistant_message_to_output_items, persisted_artifact};
use crate::stream_text::Utf8LineDecoder;
use crate::worker::json_walker::walk_json_strings;

use super::AssistantArtifact;

pub(super) const MAX_JSON_CAPTURE: usize = 1024 * 1024;

/// Issue #524 Task 4: walk every string inside the assistant artifact
/// `message_json` and apply the user-scoped redactor before persistence.
///
/// `redact_text_for_user` is already fail-open inside, so a missing runtime
/// or redactor error falls back to the original text. Any failure in the
/// walker itself is logged via `warn!` and the value is left untouched so
/// the chat-replay pipeline never aborts because of a redaction blip.
pub(super) fn redact_message_json_for_user(value: &mut Value, user_id: Option<i64>) {
    let result = walk_json_strings(value, |_, text| {
        Ok(Some(crate::redact::redact_text_for_user(text, user_id)))
    });
    if let Err(err) = result {
        warn!(
            error = %err,
            user_id = user_id.unwrap_or(0),
            "failed to walk assistant artifact message_json for redaction; leaving as-is"
        );
    }
}

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
