use serde_json::{Map, Value, json};

use crate::openai_compat::{
    assistant_message_to_output_items, persisted_artifact, reasoning_details_from_text,
};

use super::{
    AssistantArtifact, AssistantArtifactCapture,
    artifact_types::{StreamAssistantMessage, StreamToolCallState},
    shared::{
        extract_text, finish_json_capture, finish_sse_line, has_meaningful_value,
        observe_json_chunk,
    },
};

impl AssistantArtifactCapture {
    pub fn new(is_sse: bool) -> Self {
        Self {
            is_sse,
            ..Self::default()
        }
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        if self.is_sse {
            self.observe_sse_chunk(chunk);
        } else {
            observe_json_chunk(&mut self.json_body, &mut self.json_body_truncated, chunk);
        }
    }

    pub fn finish(&mut self) {
        if self.is_sse {
            if self.sse_decode_failed {
                return;
            }
            if let Some(line) = finish_sse_line(&mut self.sse_decoder) {
                self.observe_sse_line(&line);
            }
            self.finalized_message = self.stream_message.build_message();
            return;
        }
        if self.json_body_truncated {
            return;
        }
        if let Some(value) = finish_json_capture(&self.json_body) {
            self.finalized_message = extract_chat_message(&value);
        }
    }

    pub fn artifact(&self) -> Option<AssistantArtifact> {
        let assistant_message = self.finalized_message.clone()?;
        let output_items = assistant_message_to_output_items(&assistant_message).ok()?;
        let (message_json, has_reasoning_content, has_tool_calls) =
            persisted_artifact(Some(assistant_message), output_items)?;
        Some(AssistantArtifact {
            message_json,
            has_reasoning_content,
            has_tool_calls,
        })
    }

    fn observe_sse_chunk(&mut self, chunk: &[u8]) {
        if self.sse_decode_failed {
            return;
        }
        let lines = match self.sse_decoder.push(chunk) {
            Ok(lines) => lines,
            Err(_) => {
                self.sse_decode_failed = true;
                return;
            }
        };
        for line in lines {
            self.observe_sse_line(&line);
        }
    }

    fn observe_sse_line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(delta) = choice.get("delta") {
                    self.stream_message.observe_delta(delta);
                }
            }
        }
    }
}

impl StreamAssistantMessage {
    fn observe_delta(&mut self, delta: &Value) {
        if let Some(content) = delta.get("content") {
            self.content.push_str(&extract_text(content));
        }
        if let Some(refusal) = delta.get("refusal") {
            self.refusal.push_str(&extract_text(refusal));
        }
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning_details"))
        {
            self.reasoning_content.push_str(&extract_text(reasoning));
        }
        if self.phase.is_none() {
            self.phase = delta
                .get("phase")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, tool_call) in tool_calls.iter().enumerate() {
                self.observe_tool_call_delta(position, tool_call);
            }
        } else if let Some(function_call) = delta.get("function_call") {
            self.observe_legacy_function_call_delta(function_call);
        }
    }

    fn observe_tool_call_delta(&mut self, fallback_index: usize, tool_call: &Value) {
        let index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(fallback_index);
        let call_id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let position = self.resolve_tool_call_position(index, call_id.as_deref());
        let state = self
            .tool_calls
            .get_mut(position)
            .expect("tool call position resolved");
        if let Some(id) = call_id {
            state.id = Some(id);
        }
        let Some(function) = tool_call.get("function") else {
            return;
        };
        if let Some(name) = function
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            state.name = Some(name.to_string());
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            state.arguments.push_str(arguments);
        }
    }

    fn observe_legacy_function_call_delta(&mut self, function_call: &Value) {
        let position = self.resolve_tool_call_position(0, None);
        let state = self
            .tool_calls
            .get_mut(position)
            .expect("legacy tool call position resolved");
        if let Some(name) = function_call
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            state.name = Some(name.to_string());
        }
        if let Some(arguments) = function_call.get("arguments").and_then(Value::as_str) {
            state.arguments.push_str(arguments);
        }
    }

    fn build_message(&self) -> Option<Value> {
        let has_tool_calls = !self.tool_calls.is_empty();
        let has_content = !self.content.is_empty();
        let has_refusal = !self.refusal.is_empty();
        let has_reasoning = !self.reasoning_content.is_empty();
        if !has_content && !has_tool_calls && !has_refusal && !has_reasoning {
            return None;
        }
        let mut message = Map::new();
        message.insert("role".to_string(), Value::String("assistant".to_string()));
        message.insert(
            "content".to_string(),
            if has_content {
                Value::String(self.content.clone())
            } else {
                Value::Null
            },
        );
        if has_tool_calls {
            let tool_calls = self
                .tool_calls
                .iter()
                .enumerate()
                .map(|(index, state)| {
                    json!({
                        "id": state.id.clone().unwrap_or_else(|| format!("call_{index}")),
                        "type": "function",
                        "function": {
                            "name": state.name.clone().unwrap_or_default(),
                            "arguments": state.arguments,
                        }
                    })
                })
                .collect::<Vec<_>>();
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        if has_refusal {
            message.insert("refusal".to_string(), Value::String(self.refusal.clone()));
        }
        if has_reasoning {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(self.reasoning_content.clone()),
            );
            message.insert(
                "reasoning_details".to_string(),
                reasoning_details_from_text(&self.reasoning_content),
            );
        }
        if let Some(phase) = &self.phase {
            message.insert("phase".to_string(), Value::String(phase.clone()));
        }
        Some(Value::Object(message))
    }

    fn resolve_tool_call_position(&mut self, index: usize, call_id: Option<&str>) -> usize {
        if let Some(call_id) = call_id
            && let Some(position) = self
                .tool_calls
                .iter()
                .position(|state| state.id.as_deref() == Some(call_id))
        {
            self.active_tool_call_positions.insert(index, position);
            return position;
        }

        if let Some(position) = self.active_tool_call_positions.get(&index).copied() {
            let state = self
                .tool_calls
                .get(position)
                .expect("active tool call position should exist");
            if call_id.is_none() || state.id.as_deref() == call_id {
                return position;
            }
        }

        let position = self.tool_calls.len();
        self.tool_calls.push(StreamToolCallState::default());
        self.active_tool_call_positions.insert(index, position);
        position
    }
}

fn extract_chat_message(value: &Value) -> Option<Value> {
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))?;
    normalize_chat_message(message)
}

fn normalize_chat_message(message: &Value) -> Option<Value> {
    let object = message.as_object()?;
    let mut normalized = Map::new();
    normalized.insert("role".to_string(), Value::String("assistant".to_string()));

    if let Some(content) = object.get("content") {
        normalized.insert("content".to_string(), content.clone());
    }
    if let Some(tool_calls) = object.get("tool_calls") {
        if has_meaningful_value(tool_calls) {
            normalized.insert("tool_calls".to_string(), tool_calls.clone());
        }
    } else if let Some(function_call) = object.get("function_call") {
        normalized.insert(
            "tool_calls".to_string(),
            Value::Array(vec![json!({
                "id": "call_0",
                "type": "function",
                "function": function_call,
            })]),
        );
    }
    if let Some(reasoning_content) = object.get("reasoning_content")
        && has_meaningful_value(reasoning_content)
    {
        normalized.insert("reasoning_content".to_string(), reasoning_content.clone());
    }
    if let Some(reasoning_details) = object.get("reasoning_details")
        && has_meaningful_value(reasoning_details)
    {
        normalized.insert("reasoning_details".to_string(), reasoning_details.clone());
        if !normalized.contains_key("reasoning_content") {
            let extracted = extract_text(reasoning_details);
            if !extracted.trim().is_empty() {
                normalized.insert("reasoning_content".to_string(), Value::String(extracted));
            }
        }
    }
    if let Some(refusal) = object.get("refusal")
        && has_meaningful_value(refusal)
    {
        normalized.insert("refusal".to_string(), refusal.clone());
    }
    if let Some(phase) = object.get("phase")
        && has_meaningful_value(phase)
    {
        normalized.insert("phase".to_string(), phase.clone());
    }

    if !normalized.contains_key("content")
        && !normalized.contains_key("tool_calls")
        && !normalized.contains_key("refusal")
        && !normalized.contains_key("phase")
        && !normalized.contains_key("reasoning_content")
    {
        return None;
    }
    normalized
        .entry("content".to_string())
        .or_insert(Value::Null);
    Some(Value::Object(normalized))
}
