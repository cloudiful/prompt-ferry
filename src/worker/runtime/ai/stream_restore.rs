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

mod pending;
mod sse;

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
            output.push(event);
            return Ok(output);
        }
        self.restore_value(&mut value)?;
        Ok(vec![
            data_line.replace(&event, &serde_json::to_string(&value)?)?,
        ])
    }

    fn restore_value(&mut self, value: &mut Value) -> Result<()> {
        if let Some(event_type) = value.get("type").and_then(Value::as_str).map(str::to_owned) {
            match event_type.as_str() {
                "response.output_text.delta"
                | "response.reasoning_text.delta"
                | "response.function_call_arguments.delta" => {
                    let key = format!(
                        "responses:{event_type}:{}:{}:{}",
                        value.get("item_id").and_then(Value::as_str).unwrap_or(""),
                        value
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        value
                            .get("content_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                    self.restore_pointer(value, "/delta", key)?;
                    return Ok(());
                }
                "response.output_text.done" | "response.reasoning_text.done" => {
                    self.restore_complete_pointer(value, "/text")?;
                    return Ok(());
                }
                "response.function_call_arguments.done" => {
                    self.restore_complete_pointer(value, "/arguments")?;
                    return Ok(());
                }
                "content_block_start" | "content_block_delta" => {
                    self.restore_anthropic_event(value, &event_type)?;
                    return Ok(());
                }
                _ => {}
            }
        }
        self.restore_chat_event(value)
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
        validate_result(&result)?;
        let target = value.pointer_mut(pointer).expect("existing string pointer");
        *target = Value::String(result.restored_text);
        Ok(())
    }

    fn restore_complete_pointer(&self, value: &mut Value, pointer: &str) -> Result<()> {
        let Some(text) = value.pointer(pointer).and_then(Value::as_str) else {
            return Ok(());
        };
        let result = self.session.restore_state.restore_text(text)?;
        validate_result(&result)?;
        *value.pointer_mut(pointer).expect("existing string pointer") =
            Value::String(result.restored_text);
        Ok(())
    }

    fn finish_streams(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut output = Vec::new();
        for (_, stream) in self.streams.drain() {
            let result = stream.context.finish();
            validate_result(&result)?;
            if !result.restored_text.is_empty() {
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

fn validate_result(result: &redactor::RestoreResult) -> Result<()> {
    if !result.skipped_tokens.is_empty() {
        warn!(
            skipped_token_count = result.skipped_tokens.len(),
            "preserved unauthorized upstream redaction tokens"
        );
    }
    if result.is_valid() {
        Ok(())
    } else {
        Err(anyhow!(result.validation_errors.join("; ")))
    }
}

#[cfg(test)]
mod tests;
