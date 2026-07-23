use std::collections::HashMap;

use http::StatusCode;
use serde_json::{Value, json};

use super::*;
use crate::stream_text::Utf8LineDecoder;

pub struct AnthropicResponseStreamAdapter {
    sse_decoder: Utf8LineDecoder,
    response_id: Option<String>,
    model: Option<String>,
    created_at: Option<i64>,
    reasoning_id: String,
    reasoning_output_index: Option<usize>,
    message_id: String,
    message_output_index: Option<usize>,
    full_reasoning_text: String,
    full_text: String,
    usage: Option<Value>,
    created_emitted: bool,
    completed: bool,
    next_output_index: usize,
    next_sequence_number: usize,
    tool_calls: Vec<StreamToolCallState>,
    content_block_positions: HashMap<usize, AnthropicContentBlockPosition>,
}

#[derive(Clone, Copy)]
enum AnthropicContentBlockPosition {
    Reasoning,
    Text,
    ToolCall(usize),
}

impl Default for AnthropicResponseStreamAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicResponseStreamAdapter {
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
            completed: false,
            next_output_index: 0,
            next_sequence_number: 0,
            tool_calls: Vec::new(),
            content_block_positions: HashMap::new(),
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
                "anthropic-native endpoint returned invalid streaming JSON",
            )
        })?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "message_start" => self.handle_message_start(&value, output)?,
            "content_block_start" => self.handle_content_block_start(&value, output)?,
            "content_block_delta" => self.handle_content_block_delta(&value, output)?,
            "message_delta" => self.handle_message_delta(&value)?,
            "message_stop" => self.emit_completion(output)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_message_start(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let Some(message) = value.get("message") else {
            return Ok(());
        };
        self.response_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.is_empty());
        self.model = message
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.is_empty());
        self.created_at = Some(chrono::Utc::now().timestamp());
        if let Some(usage) = message
            .get("usage")
            .and_then(anthropic_stream_usage_to_openai_usage)
        {
            self.usage = Some(usage);
        }
        self.ensure_response_created(output)
    }

    fn handle_content_block_start(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        self.ensure_response_created(output)?;
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;
        let Some(content_block) = value.get("content_block").and_then(Value::as_object) else {
            return Ok(());
        };
        match content_block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "thinking" => {
                let output_index = self.ensure_reasoning_stream_started(output)?;
                self.content_block_positions
                    .insert(index, AnthropicContentBlockPosition::Reasoning);
                if let Some(thinking) = content_block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    self.full_reasoning_text.push_str(thinking);
                    self.push_event(
                        output,
                        json!({
                            "type": "response.reasoning_text.delta",
                            "output_index": output_index,
                            "content_index": 0,
                            "item_id": self.reasoning_id,
                            "delta": thinking,
                        }),
                    )?;
                }
            }
            "text" => {
                let output_index = self.ensure_text_stream_started(output)?;
                self.content_block_positions
                    .insert(index, AnthropicContentBlockPosition::Text);
                if let Some(text) = content_block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    self.full_text.push_str(text);
                    self.push_event(
                        output,
                        json!({
                            "type": "response.output_text.delta",
                            "output_index": output_index,
                            "content_index": 0,
                            "item_id": self.message_id,
                            "delta": text,
                            "logprobs": [],
                        }),
                    )?;
                }
            }
            "tool_use" => {
                let output_index = self.allocate_output_index();
                let call_id = content_block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(generate_call_id);
                let name = content_block
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|value| !value.is_empty());
                let position = self.tool_calls.len();
                let mut state = StreamToolCallState::new(call_id.clone(), output_index);
                state.name = name.clone();
                if let Some(input) = content_block.get("input")
                    && !matches!(input, Value::Object(object) if object.is_empty())
                {
                    state.arguments =
                        serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                }
                self.tool_calls.push(state);
                self.content_block_positions
                    .insert(index, AnthropicContentBlockPosition::ToolCall(position));
                self.push_event(
                    output,
                    json!({
                        "type": "response.output_item.added",
                        "output_index": output_index,
                        "item": function_call_item(
                            &call_id,
                            name.as_deref().unwrap_or(""),
                            "",
                            "in_progress",
                        ),
                    }),
                )?;
                if let Some(input) = content_block.get("input")
                    && let Ok(arguments) = serde_json::to_string(input)
                    && arguments != "{}"
                {
                    if let Some(state) = self.tool_calls.get_mut(position) {
                        state.arguments = arguments.clone();
                    }
                    self.push_event(
                        output,
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "response_id": self.current_response_id(),
                            "item_id": call_id.clone(),
                            "output_index": output_index,
                            "call_id": call_id,
                            "delta": arguments,
                        }),
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_content_block_delta(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;
        let Some(delta) = value.get("delta").and_then(Value::as_object) else {
            return Ok(());
        };
        let Some(position) = self.content_block_positions.get(&index).copied() else {
            return Ok(());
        };
        match position {
            AnthropicContentBlockPosition::Reasoning => {
                let text = delta
                    .get("thinking")
                    .or_else(|| delta.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_empty() {
                    return Ok(());
                }
                let output_index = self.ensure_reasoning_stream_started(output)?;
                self.full_reasoning_text.push_str(text);
                self.push_event(
                    output,
                    json!({
                        "type": "response.reasoning_text.delta",
                        "output_index": output_index,
                        "content_index": 0,
                        "item_id": self.reasoning_id,
                        "delta": text,
                    }),
                )?;
            }
            AnthropicContentBlockPosition::Text => {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_empty() {
                    return Ok(());
                }
                let output_index = self.ensure_text_stream_started(output)?;
                self.full_text.push_str(text);
                self.push_event(
                    output,
                    json!({
                        "type": "response.output_text.delta",
                        "output_index": output_index,
                        "content_index": 0,
                        "item_id": self.message_id,
                        "delta": text,
                        "logprobs": [],
                    }),
                )?;
            }
            AnthropicContentBlockPosition::ToolCall(position) => {
                let arguments_delta = delta
                    .get("partial_json")
                    .or_else(|| delta.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if arguments_delta.is_empty() {
                    return Ok(());
                }
                let Some(state) = self.tool_calls.get_mut(position) else {
                    return Ok(());
                };
                state.arguments.push_str(arguments_delta);
                let call_id = state.call_id.clone();
                let output_index = state.output_index;
                let response_id = self.current_response_id();
                self.push_event(
                    output,
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "response_id": response_id,
                        "item_id": call_id.clone(),
                        "output_index": output_index,
                        "call_id": call_id,
                        "delta": arguments_delta,
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn handle_message_delta(&mut self, value: &Value) -> Result<(), CompatError> {
        if let Some(next_usage) = value
            .get("usage")
            .and_then(anthropic_stream_usage_to_openai_usage)
        {
            self.usage = Some(merge_openai_usage(self.usage.take(), next_usage));
        }
        Ok(())
    }

    fn ensure_response_created(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
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
    ) -> Result<usize, CompatError> {
        self.ensure_response_created(output)?;
        let output_index = if let Some(output_index) = self.reasoning_output_index {
            output_index
        } else {
            let output_index = self.allocate_output_index();
            self.reasoning_output_index = Some(output_index);
            output_index
        };
        if self.reasoning_output_index == Some(output_index) && self.full_reasoning_text.is_empty()
        {
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": reasoning_item_with_status(&self.reasoning_id, "", "in_progress"),
                }),
            )?;
        }
        Ok(output_index)
    }

    fn ensure_text_stream_started(
        &mut self,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<usize, CompatError> {
        self.ensure_response_created(output)?;
        let output_index = if let Some(output_index) = self.message_output_index {
            output_index
        } else {
            let output_index = self.allocate_output_index();
            self.message_output_index = Some(output_index);
            output_index
        };
        if self.message_output_index == Some(output_index) && self.full_text.is_empty() {
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
        }
        Ok(output_index)
    }

    fn emit_completion(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        if self.completed {
            return Ok(());
        }
        self.ensure_response_created(output)?;

        if let Some(output_index) = self.reasoning_output_index
            && !self.full_reasoning_text.is_empty()
        {
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
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": reasoning_item_with_status(
                        &self.reasoning_id,
                        &self.full_reasoning_text,
                        "completed",
                    ),
                }),
            )?;
        }

        if let Some(output_index) = self.message_output_index {
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
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": message_item_with_status(&self.message_id, &self.full_text, "completed"),
                }),
            )?;
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

    fn emit_pending_tool_completions(
        &mut self,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let response_id = self.current_response_id();
        let mut completed = Vec::new();
        for state in &mut self.tool_calls {
            if state.done_emitted {
                continue;
            }
            let arguments = normalize_partial_json_arguments(&state.arguments);
            state.arguments = arguments.clone();
            state.done_emitted = true;
            completed.push((
                state.call_id.clone(),
                state.name.clone().unwrap_or_default(),
                state.output_index,
                arguments,
            ));
        }
        for (call_id, name, output_index, arguments) in completed {
            self.push_event(
                output,
                json!({
                    "type": "response.function_call_arguments.done",
                    "response_id": response_id,
                    "item_id": call_id.clone(),
                    "output_index": output_index,
                    "call_id": call_id.clone(),
                    "name": name.clone(),
                    "arguments": arguments.clone(),
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": function_call_item(
                        &call_id,
                        &name,
                        &arguments,
                        "completed",
                    ),
                }),
            )?;
        }
        Ok(())
    }

    fn current_output_items(&self) -> Vec<Value> {
        let mut items = Vec::new();
        if let Some(output_index) = self.reasoning_output_index
            && !self.full_reasoning_text.is_empty()
        {
            items.push((
                output_index,
                reasoning_item_with_status(
                    &self.reasoning_id,
                    &self.full_reasoning_text,
                    "completed",
                ),
            ));
        }
        if let Some(output_index) = self.message_output_index {
            items.push((
                output_index,
                message_item_with_status(&self.message_id, &self.full_text, "completed"),
            ));
        }
        for state in &self.tool_calls {
            items.push((
                state.output_index,
                function_call_item(
                    &state.call_id,
                    state.name.as_deref().unwrap_or(""),
                    &state.arguments,
                    "completed",
                ),
            ));
        }
        items.sort_by_key(|(output_index, _)| *output_index);
        items.into_iter().map(|(_, item)| item).collect()
    }

    fn allocate_output_index(&mut self) -> usize {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        output_index
    }

    fn current_response_id(&self) -> String {
        self.response_id
            .clone()
            .unwrap_or_else(generate_response_id)
    }

    fn push_event(
        &mut self,
        output: &mut Vec<Vec<u8>>,
        mut event: Value,
    ) -> Result<(), CompatError> {
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "sequence_number".to_string(),
                Value::from(self.next_sequence_number as u64),
            );
        }
        self.next_sequence_number += 1;
        output.push(sse_event(&event)?);
        Ok(())
    }
}

fn normalize_partial_json_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return "{}".to_string();
    }
    if serde_json::from_str::<Value>(arguments).is_ok() {
        return arguments.to_string();
    }
    if let Ok(value) = serde_json::from_str::<Value>(&format!("{{{arguments}}}")) {
        return serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    }
    "{}".to_string()
}

fn anthropic_stream_usage_to_openai_usage(usage: &Value) -> Option<Value> {
    let input_tokens = usage.get("input_tokens").and_then(Value::as_i64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_i64);
    let cache_read_tokens = usage.get("cache_read_input_tokens").and_then(Value::as_i64);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64);
    if [
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    ]
    .into_iter()
    .all(|value| value.is_none())
    {
        return None;
    }
    let input_tokens = input_tokens.unwrap_or_default();
    let output_tokens = output_tokens.unwrap_or_default();
    let cache_read_tokens = cache_read_tokens.unwrap_or_default();
    let cache_write_tokens = cache_write_tokens.unwrap_or_default();
    let total_input_tokens = input_tokens + cache_read_tokens + cache_write_tokens;
    Some(json!({
        "input_tokens": total_input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_input_tokens + output_tokens,
        "input_tokens_details": {
            "cached_tokens": cache_read_tokens,
            "cache_read_tokens": cache_read_tokens,
            "cache_write_tokens": cache_write_tokens,
        },
        "output_tokens_details": {
            "reasoning_tokens": 0,
        },
    }))
}

fn merge_openai_usage(current: Option<Value>, next: Value) -> Value {
    let current = current.unwrap_or_else(default_response_usage);
    json!({
        "input_tokens": next["input_tokens"].as_i64().unwrap_or(0).max(current["input_tokens"].as_i64().unwrap_or(0)),
        "output_tokens": next["output_tokens"].as_i64().unwrap_or(0).max(current["output_tokens"].as_i64().unwrap_or(0)),
        "total_tokens": next["total_tokens"].as_i64().unwrap_or(0).max(current["total_tokens"].as_i64().unwrap_or(0)),
        "input_tokens_details": {
            "cached_tokens": next["input_tokens_details"]["cache_read_tokens"]
                .as_i64()
                .unwrap_or(0)
                .max(current["input_tokens_details"]["cache_read_tokens"].as_i64().unwrap_or(0)),
            "cache_read_tokens": next["input_tokens_details"]["cache_read_tokens"]
                .as_i64()
                .unwrap_or(0)
                .max(current["input_tokens_details"]["cache_read_tokens"].as_i64().unwrap_or(0)),
            "cache_write_tokens": next["input_tokens_details"]["cache_write_tokens"]
                .as_i64()
                .unwrap_or(0)
                .max(current["input_tokens_details"]["cache_write_tokens"].as_i64().unwrap_or(0)),
        },
        "output_tokens_details": {
            "reasoning_tokens": next["output_tokens_details"]["reasoning_tokens"]
                .as_i64()
                .unwrap_or(0)
                .max(current["output_tokens_details"]["reasoning_tokens"].as_i64().unwrap_or(0)),
        },
    })
}
