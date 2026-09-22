use std::collections::HashMap;

use anyhow::{Result, anyhow};
use redactor::StreamingRestoreContext;
use serde_json::Value;
use tracing::warn;

use crate::{
    redact_upstream::UpstreamRedactionSession,
    worker::runtime::error_handling::{PassthroughSseFilter, ResponsesSseTerminal},
};

use self::sse::DataLine;
use super::upstream_restore::log_restore_diagnostics;

mod pending;
mod sse;

/// Prefix of every upstream redaction token; a payload carrying it must never
/// reach the client unredacted-or-unrestored.
const TOKEN_PREFIX: &str = "[[RDX:v2:";

struct TextStream<'a> {
    context: StreamingRestoreContext<'a>,
    template: Value,
    pointer: String,
}

pub(super) struct SseRestoreFilter<'a> {
    events: PassthroughSseFilter,
    session: &'a UpstreamRedactionSession,
    streams: HashMap<String, TextStream<'a>>,
}

impl<'a> SseRestoreFilter<'a> {
    pub(super) fn new(session: &'a UpstreamRedactionSession) -> Self {
        Self::with_responses_terminal(session, false)
    }

    pub(super) fn new_responses(session: &'a UpstreamRedactionSession) -> Self {
        Self::with_responses_terminal(session, true)
    }

    fn with_responses_terminal(
        session: &'a UpstreamRedactionSession,
        responses_terminal: bool,
    ) -> Self {
        Self {
            events: if responses_terminal {
                PassthroughSseFilter::new_responses()
            } else {
                PassthroughSseFilter::new()
            },
            session,
            streams: HashMap::new(),
        }
    }

    pub(super) fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>> {
        let events = self
            .events
            .push_chunk(chunk)
            .unwrap_or_else(|err| match err {});
        self.restore_events(events)
    }

    pub(super) fn push_chunks(&mut self, chunks: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>> {
        let mut output = Vec::new();
        for chunk in chunks {
            output.extend(self.push_chunk(&chunk)?);
        }
        Ok(output)
    }

    pub(super) fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        let events = self.events.finish().unwrap_or_else(|err| match err {});
        let mut output = self.restore_events(events)?;
        output.extend(self.finish_streams()?);
        Ok(output)
    }

    pub(super) fn is_done(&self) -> bool {
        self.events.is_done()
    }

    pub(super) fn responses_terminal(&self) -> Option<ResponsesSseTerminal> {
        self.events.responses_terminal()
    }

    pub(super) fn responses_error_body(&self) -> Option<&str> {
        self.events.responses_error_body()
    }

    fn restore_events(&mut self, events: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>> {
        let mut output = Vec::new();
        for event in events {
            output.extend(self.restore_event(event)?);
        }
        Ok(output)
    }

    fn restore_event(&mut self, event: Vec<u8>) -> Result<Vec<Vec<u8>>> {
        let Some(data_line) = DataLine::parse(&event)? else {
            return Ok(vec![event]);
        };
        if data_line.payload == "[DONE]" {
            let mut output = self.finish_streams()?;
            output.push(event);
            return Ok(output);
        }
        let mut value: Value = serde_json::from_str(data_line.payload)
            .map_err(|err| anyhow!("invalid SSE data JSON during upstream restore: {err}"))?;
        if is_terminal_event(&value) {
            let mut output = self.finish_streams()?;
            let original = value.clone();
            self.restore_value(&mut value)?;
            warn_residual_tokens(&value, "ai_sse");
            // Terminal events were historically forwarded byte-identically; only
            // rewrite them when a snapshot field actually changed.
            if original == value {
                output.push(event);
            } else {
                output.push(data_line.replace(&event, &serde_json::to_string(&value)?)?);
            }
            return Ok(output);
        }
        self.restore_value(&mut value)?;
        warn_residual_tokens(&value, "ai_sse");
        Ok(vec![
            data_line.replace(&event, &serde_json::to_string(&value)?)?,
        ])
    }

    /// Restore every token-bearing field this event family is known to carry.
    ///
    /// Events whose type is not in the whitelist fall through to
    /// `restore_chat_event`; the caller reports any token still present
    /// afterwards via [`warn_residual_tokens`].
    fn restore_value(&mut self, value: &mut Value) -> Result<()> {
        let Some(event_type) = value.get("type").and_then(Value::as_str).map(str::to_owned) else {
            self.restore_chat_event(value)?;
            return Ok(());
        };
        match event_type.as_str() {
            "response.output_text.delta"
            | "response.reasoning_text.delta"
            | "response.function_call_arguments.delta"
            | "response.reasoning_summary_text.delta" => {
                let key = response_stream_key(&event_type, value);
                self.restore_pointer(value, "/delta", key)?;
            }
            "response.output_text.done"
            | "response.reasoning_text.done"
            | "response.reasoning_summary_text.done" => {
                self.restore_complete_pointer(value, "/text")?;
            }
            "response.function_call_arguments.done" => {
                self.restore_complete_pointer(value, "/arguments")?;
            }
            "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
                self.restore_complete_pointer(value, "/part/text")?;
            }
            "response.content_part.added" | "response.content_part.done" => {
                self.restore_complete_pointer(value, "/part/text")?;
            }
            "response.output_item.added" | "response.output_item.done" => {
                self.restore_output_item(value)?;
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                self.restore_terminal_snapshot(value)?;
            }
            "content_block_start" | "content_block_delta" => {
                self.restore_anthropic_event(value, &event_type)?;
            }
            _ => self.restore_chat_event(value)?,
        }
        Ok(())
    }

    /// Restore the text-bearing fields carried by a terminal replies event.
    /// Reasoning items expose their summary as `summary[*].text` (and the
    /// reasoning body as `content[*].text`), message items expose output text
    /// as `content[*].text`, function calls expose `arguments`; the walk
    /// mirrors the non-stream Responses allowlist so both paths cover the
    /// same fields.
    fn restore_terminal_snapshot(&self, value: &mut Value) -> Result<()> {
        let response = match value.get_mut("response") {
            Some(response) => response,
            None => value,
        };
        let Some(output) = response.get_mut("output") else {
            return Ok(());
        };
        self.restore_snapshot_value(output)
    }

    fn restore_snapshot_value(&self, value: &mut Value) -> Result<()> {
        match value {
            Value::Array(items) => {
                for item in items.iter_mut() {
                    self.restore_snapshot_value(item)?;
                }
            }
            Value::Object(object) => {
                for (key, child) in object.iter_mut() {
                    if snapshot_field_is_text(key) && child.is_string() {
                        self.restore_complete_value(child)?;
                        continue;
                    }
                    self.restore_snapshot_value(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Restore the text-bearing fields inside a single Responses output item,
    /// as carried by `response.output_item.added|done`. The walk reuses the
    /// terminal snapshot field set so every restore path covers the same
    /// fields.
    fn restore_output_item(&self, value: &mut Value) -> Result<()> {
        let Some(item) = value.get_mut("item") else {
            return Ok(());
        };
        self.restore_snapshot_value(item)
    }

    fn restore_complete_value(&self, value: &mut Value) -> Result<()> {
        let Some(text) = value.as_str() else {
            return Ok(());
        };
        let result = self.session.restore_state.restore_text(text)?;
        log_restore_diagnostics(&result, "ai_sse");
        *value = Value::String(result.restored_text);
        Ok(())
    }

    fn restore_anthropic_event(&mut self, value: &mut Value, event_type: &str) -> Result<()> {
        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
        let candidates = if event_type == "content_block_start" {
            [
                ("/content_block/text", "text"),
                ("/content_block/thinking", "thinking"),
                ("/content_block/partial_json", "arguments"),
            ]
        } else {
            [
                ("/delta/text", "text"),
                ("/delta/thinking", "thinking"),
                ("/delta/partial_json", "arguments"),
            ]
        };
        for (pointer, kind) in candidates {
            self.restore_pointer(value, pointer, format!("anthropic:{index}:{kind}"))?;
        }
        Ok(())
    }

    fn restore_chat_event(&mut self, value: &mut Value) -> Result<()> {
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return Ok(());
        };
        let choice_indexes = choices
            .iter()
            .enumerate()
            .map(|(array_index, choice)| {
                (
                    array_index,
                    choice
                        .get("index")
                        .and_then(Value::as_u64)
                        .unwrap_or(array_index as u64),
                )
            })
            .collect::<Vec<_>>();
        for (array_index, choice_index) in choice_indexes {
            for field in ["content", "reasoning_content", "reasoning", "refusal"] {
                self.restore_pointer(
                    value,
                    &format!("/choices/{array_index}/delta/{field}"),
                    format!("chat:{choice_index}:{field}"),
                )?;
            }
            let detail_count = value
                .pointer(&format!("/choices/{array_index}/delta/reasoning_details"))
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            for detail_index in 0..detail_count {
                self.restore_pointer(
                    value,
                    &format!("/choices/{array_index}/delta/reasoning_details/{detail_index}/text"),
                    format!("chat:{choice_index}:reasoning_details:{detail_index}"),
                )?;
            }
            let tool_count = value
                .pointer(&format!("/choices/{array_index}/delta/tool_calls"))
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            for tool_array_index in 0..tool_count {
                let tool_index = value
                    .pointer(&format!(
                        "/choices/{array_index}/delta/tool_calls/{tool_array_index}/index"
                    ))
                    .and_then(Value::as_u64)
                    .unwrap_or(tool_array_index as u64);
                self.restore_pointer(
                    value,
                    &format!(
                        "/choices/{array_index}/delta/tool_calls/{tool_array_index}/function/arguments"
                    ),
                    format!("chat:{choice_index}:tool:{tool_index}:arguments"),
                )?;
            }
        }
        Ok(())
    }

    fn restore_pointer(&mut self, value: &mut Value, pointer: &str, key: String) -> Result<()> {
        let Some(text) = value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Ok(());
        };
        let template = value.clone();
        let stream = match self.streams.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(TextStream {
                context: self.session.restore_state.streaming_restore_context()?,
                template: template.clone(),
                pointer: pointer.to_string(),
            }),
        };
        stream.template = template;
        stream.pointer.clear();
        stream.pointer.push_str(pointer);
        let result = stream.context.push_str(&text);
        log_restore_diagnostics(&result, "ai_sse");
        let target = value.pointer_mut(pointer).expect("existing string pointer");
        *target = Value::String(result.restored_text);
        Ok(())
    }

    fn restore_complete_pointer(&self, value: &mut Value, pointer: &str) -> Result<()> {
        let Some(target) = value.pointer_mut(pointer) else {
            return Ok(());
        };
        self.restore_complete_value(target)
    }

    fn finish_streams(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut output = Vec::new();
        for (_, stream) in self.streams.drain() {
            let result = stream.context.finish();
            log_restore_diagnostics(&result, "ai_sse");
            if !result.restored_text.is_empty() {
                if result.restored_text.contains(TOKEN_PREFIX) {
                    warn!(
                        event = "restore_unhandled_field",
                        restore_surface = "ai_sse",
                        event_type = "pending_stream",
                        field_path = stream.pointer.as_str(),
                        token_prefix = token_prefix(&result.restored_text),
                        "upstream redaction token reached the client unhandled"
                    );
                }
                output.push(pending::synthetic_delta(
                    stream.template,
                    &stream.pointer,
                    result.restored_text,
                )?);
            }
        }
        Ok(output)
    }
}

fn is_terminal_event(value: &Value) -> bool {
    matches!(
        value.get("type").and_then(Value::as_str),
        Some("response.completed")
            | Some("response.failed")
            | Some("response.incomplete")
            | Some("error")
            | Some("message_stop")
    )
}

/// Text-bearing fields inside a terminal `response.output` snapshot, mirroring
/// the non-stream Responses allowlist (`upstream_text_fields.rs`) so both
/// restore paths cover the same fields.
fn snapshot_field_is_text(key: &str) -> bool {
    matches!(
        key,
        "text"
            | "arguments"
            | "output"
            | "summary"
            | "content"
            | "reasoning_content"
            | "reasoning"
            | "reasoning_text"
            | "thinking"
            | "chain_of_thought"
            | "refusal"
    )
}

fn response_stream_key(event_type: &str, value: &Value) -> String {
    format!(
        "responses:{event_type}:{}:{}:{}:{}",
        value.get("item_id").and_then(Value::as_str).unwrap_or(""),
        value
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        value
            .get("content_index")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        value
            .get("summary_index")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
}

/// A short, stable identifier for the redaction token beginning at the first
/// `[[RDX:v2:` occurrence: the marker plus a bounded slice of the payload, so
/// the full ciphertext never lands in the logs.
fn token_prefix(text: &str) -> &str {
    text.find(TOKEN_PREFIX).map_or(TOKEN_PREFIX, |start| {
        let end = (start + TOKEN_PREFIX.len() + 8).min(text.len());
        &text[start..end]
    })
}

fn token_bearing_paths(value: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_token_paths(value, &mut String::new(), &mut paths);
    paths
}

fn collect_token_paths(value: &Value, path: &mut String, paths: &mut Vec<String>) {
    match value {
        Value::String(text) if text.contains(TOKEN_PREFIX) => paths.push(path.clone()),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("/{index}"));
                collect_token_paths(item, path, paths);
                path.truncate(len);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let len = path.len();
                path.push('/');
                path.push_str(key);
                collect_token_paths(item, path, paths);
                path.truncate(len);
            }
        }
        _ => {}
    }
}

/// Warn about any redaction token still present in a restored event. A token
/// that survived the whitelist means the event family is either unknown or
/// only partially covered; the bytes are forwarded so the stream keeps
/// flowing, but the leak is at least visible in the logs.
fn warn_residual_tokens(value: &Value, surface: &'static str) {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut paths = token_bearing_paths(value);
    if paths.is_empty() {
        return;
    }
    paths.sort();
    for field_path in paths {
        let token_prefix = value
            .pointer(&field_path)
            .and_then(Value::as_str)
            .map_or(TOKEN_PREFIX, token_prefix);
        warn!(
            event = "restore_unhandled_field",
            restore_surface = surface,
            event_type,
            field_path = field_path.as_str(),
            token_prefix,
            "upstream redaction token reached the client unhandled"
        );
    }
}

#[cfg(test)]
mod tests;
