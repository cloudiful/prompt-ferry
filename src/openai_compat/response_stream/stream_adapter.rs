use super::*;
use std::collections::BTreeMap;

use crate::stream_text::Utf8LineDecoder;

pub struct ChatResponseStreamAdapter {
    pub(super) sse_decoder: Utf8LineDecoder,
    pub(super) response_id: Option<String>,
    pub(super) model: Option<String>,
    pub(super) created_at: Option<i64>,
    pub(super) reasoning_id: String,
    pub(super) reasoning_output_index: Option<usize>,
    pub(super) message_id: String,
    pub(super) message_output_index: Option<usize>,
    pub(super) full_reasoning_text: String,
    pub(super) full_text: String,
    pub(super) usage: Option<Value>,
    pub(super) created_emitted: bool,
    pub(super) reasoning_started: bool,
    pub(super) content_started: bool,
    pub(super) completed: bool,
    pub(super) next_output_index: usize,
    pub(super) next_sequence_number: usize,
    pub(super) tool_calls: Vec<StreamToolCallState>,
    pub(super) active_tool_call_positions: BTreeMap<usize, usize>,
}

impl Default for ChatResponseStreamAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatResponseStreamAdapter {
    const REASONING_SUMMARY_INDEX: usize = 0;

    pub fn new() -> Self {
        Self {
            sse_decoder: Utf8LineDecoder::default(),
            response_id: None,
            model: None,
            created_at: None,
            reasoning_id: generate_reasoning_id(),
            reasoning_output_index: None,
            message_id: generate_message_id(),
            message_output_index: None,
            full_reasoning_text: String::new(),
            full_text: String::new(),
            usage: None,
            created_emitted: false,
            reasoning_started: false,
            content_started: false,
            completed: false,
            next_output_index: 0,
            next_sequence_number: 0,
            tool_calls: Vec::new(),
            active_tool_call_positions: BTreeMap::new(),
        }
    }

    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CompatError> {
        let mut output = Vec::new();
        for line in super::decode_sse_chunk(&mut self.sse_decoder, chunk)? {
            self.process_line(&line, &mut output)?;
        }
        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, CompatError> {
        let mut output = Vec::new();
        if let Some(line) = super::finish_sse_decoder(&mut self.sse_decoder)?
            && !line.trim().is_empty()
        {
            self.process_line(line.trim_end_matches(['\r', '\n']), &mut output)?;
        }
        if !self.completed {
            self.emit_completion(&mut output)?;
        }
        Ok(output)
    }

    pub fn provider_response_id(&self) -> Option<&str> {
        self.response_id.as_deref()
    }

    pub fn model_name(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn process_line(&mut self, line: &str, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(());
        };
        let data = data.trim();
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.emit_completion(output)?;
            return Ok(());
        }
        let value = serde_json::from_str::<Value>(data).map_err(|_| {
            CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "chat-native endpoint returned invalid streaming JSON",
            )
        })?;

        self.observe_metadata(&value);
        if let Some(usage) = usage_from_chat_value(&value) {
            self.usage = Some(usage);
        }
        for tool_call in extract_chat_delta_tool_calls(&value)? {
            self.observe_tool_call_delta(tool_call, output)?;
        }
        let reasoning_delta = extract_chat_delta_reasoning_text(&value);
        if !reasoning_delta.is_empty() {
            self.ensure_reasoning_stream_started(output)?;
            self.full_reasoning_text.push_str(&reasoning_delta);
            self.push_event(
                output,
                json!({
                    "type": "response.reasoning_text.delta",
                    "output_index": self.reasoning_output_index.unwrap_or_default(),
                    "content_index": 0,
                    "item_id": self.reasoning_id,
                    "delta": reasoning_delta,
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "output_index": self.reasoning_output_index.unwrap_or_default(),
                    "summary_index": Self::REASONING_SUMMARY_INDEX,
                    "item_id": self.reasoning_id,
                    "delta": reasoning_delta,
                }),
            )?;
        }
        let text_delta = extract_chat_delta_text(&value);
        if !text_delta.is_empty() {
            self.ensure_text_stream_started(output)?;
            self.full_text.push_str(&text_delta);
            self.push_event(
                output,
                json!({
                    "type": "response.output_text.delta",
                    "output_index": self.message_output_index.unwrap_or_default(),
                    "content_index": 0,
                    "item_id": self.message_id,
                    "delta": text_delta,
                    "logprobs": [],
                }),
            )?;
        }
        Ok(())
    }

    fn observe_metadata(&mut self, value: &Value) {
        if self.response_id.is_none() {
            self.response_id = value
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|value| !value.is_empty());
        }
        if self.model.is_none() {
            self.model = value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|value| !value.is_empty());
        }
        if self.created_at.is_none() {
            self.created_at = value.get("created").and_then(Value::as_i64);
        }
    }

    pub(super) fn ensure_response_created(
        &mut self,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        if self.created_emitted {
            return Ok(());
        }
        self.push_event(
            output,
            json!({
                "type": "response.created",
                "response": response_shell(
                    self.current_response_id(),
                    self.model.as_deref(),
                    self.created_at,
                    "in_progress",
                ),
            }),
        )?;
        self.created_emitted = true;
        Ok(())
    }

    fn ensure_reasoning_stream_started(
        &mut self,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        self.ensure_response_created(output)?;
        if !self.reasoning_started {
            let output_index = if let Some(output_index) = self.reasoning_output_index {
                output_index
            } else {
                let output_index = self.allocate_output_index();
                self.reasoning_output_index = Some(output_index);
                output_index
            };
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": reasoning_item_with_status(&self.reasoning_id, "", "in_progress"),
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.reasoning_summary_part.added",
                    "output_index": output_index,
                    "summary_index": Self::REASONING_SUMMARY_INDEX,
                    "item_id": self.reasoning_id,
                    "part": {
                        "type": "summary_text",
                        "text": "",
                    },
                }),
            )?;
            self.reasoning_started = true;
        }
        Ok(())
    }

    fn ensure_text_stream_started(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        self.ensure_response_created(output)?;
        if !self.content_started {
            let output_index = if let Some(output_index) = self.message_output_index {
                output_index
            } else {
                let output_index = self.allocate_output_index();
                self.message_output_index = Some(output_index);
                output_index
            };
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": message_item_with_status(&self.message_id, "", "in_progress"),
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.content_part.added",
                    "output_index": output_index,
                    "content_index": 0,
                    "item_id": self.message_id,
                    "part": {
                        "type": "output_text",
                        "text": "",
                        "annotations": [],
                        "logprobs": [],
                    },
                }),
            )?;
            self.content_started = true;
        }
        Ok(())
    }

    fn emit_completion(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        if self.completed {
            return Ok(());
        }
        self.ensure_response_created(output)?;

        if self.reasoning_started {
            let output_index = self.reasoning_output_index.unwrap_or_default();
            self.push_event(
                output,
                json!({
                    "type": "response.reasoning_text.done",
                    "output_index": output_index,
                    "content_index": 0,
                    "item_id": self.reasoning_id,
                    "text": self.full_reasoning_text,
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "output_index": output_index,
                    "summary_index": Self::REASONING_SUMMARY_INDEX,
                    "item_id": self.reasoning_id,
                    "part": {
                        "type": "summary_text",
                        "text": self.full_reasoning_text,
                    },
                }),
            )?;
            self.push_event(output, json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": reasoning_item_with_status(&self.reasoning_id, &self.full_reasoning_text, "completed"),
            }))?;
        }

        if self.content_started {
            let output_index = self.message_output_index.unwrap_or_default();
            self.push_event(
                output,
                json!({
                    "type": "response.output_text.done",
                    "output_index": output_index,
                    "content_index": 0,
                    "item_id": self.message_id,
                    "text": self.full_text,
                    "logprobs": [],
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.content_part.done",
                    "output_index": output_index,
                    "content_index": 0,
                    "item_id": self.message_id,
                    "part": {
                        "type": "output_text",
                        "text": self.full_text,
                        "annotations": [],
                        "logprobs": [],
                    },
                }),
            )?;
            self.push_event(output, json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": message_item_with_status(&self.message_id, &self.full_text, "completed"),
            }))?;
        }

        self.emit_pending_tool_completions(output)?;

        self.push_event(
            output,
            json!({
                "type": "response.completed",
                "response": build_response_object(
                    self.current_response_id(),
                    self.model.as_deref(),
                    self.created_at,
                    self.current_output_items(),
                    self.usage.clone(),
                    "completed",
                ),
            }),
        )?;
        self.completed = true;
        Ok(())
    }
}
