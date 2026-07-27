use serde_json::{Map, Value, json};

use super::super::sse_event;
use super::{ResponsesChatResponseStreamAdapter, ToolCall};
use crate::openai_compat::{CompatError, extract_text};

impl ResponsesChatResponseStreamAdapter {
    pub(super) fn ensure_created(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        if self.created_emitted {
            return Ok(());
        }
        self.emit_chat_chunk(
            output,
            json!({
                "role": "assistant",
                "content": Value::Null,
            }),
            None,
            None,
        )?;
        self.created_emitted = true;
        Ok(())
    }

    pub(super) fn emit_text_delta(
        &mut self,
        delta: &str,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        if delta.is_empty() {
            return Ok(());
        }
        self.ensure_created(output)?;
        self.full_text.push_str(delta);
        self.emit_chat_chunk(output, json!({"content": delta}), None, None)
    }

    pub(super) fn emit_reasoning_delta(
        &mut self,
        delta: &str,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        if delta.is_empty() {
            return Ok(());
        }
        self.ensure_created(output)?;
        self.full_reasoning.push_str(delta);
        self.emit_chat_chunk(output, json!({"reasoning_content": delta}), None, None)
    }

    pub(super) fn observe_output_item(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let output_index = value
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let item = value.get("item").unwrap_or(value);
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return Ok(());
        }
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (position, is_new) = self.ensure_tool_call(output_index, call_id, &name);
        if !name.is_empty() {
            self.tool_calls[position].name = name;
        }
        self.emit_tool_call_start(position, is_new, output)?;
        if !arguments.is_empty() {
            let delta = self.merge_full_arguments(position, &arguments);
            if !delta.is_empty() {
                self.emit_chat_chunk(
                    output,
                    json!({
                        "tool_calls": [{
                            "index": position,
                            "function": {"arguments": delta}
                        }]
                    }),
                    None,
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn observe_function_arguments_delta(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let output_index = value
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let call_id = value
            .get("call_id")
            .or_else(|| value.get("item_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let (position, is_new) = self.ensure_tool_call(output_index, call_id, "");
        self.emit_tool_call_start(position, is_new, output)?;
        if !delta.is_empty() {
            self.tool_calls[position].arguments.push_str(delta);
            self.tool_calls[position].emitted_arguments = self.tool_calls[position].arguments.len();
            self.emit_chat_chunk(
                output,
                json!({
                    "tool_calls": [{
                        "index": position,
                        "function": {"arguments": delta}
                    }]
                }),
                None,
                None,
            )?;
        }
        Ok(())
    }

    pub(super) fn observe_function_arguments_done(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let output_index = value
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let call_id = value
            .get("call_id")
            .or_else(|| value.get("item_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let arguments = value
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (position, is_new) = self.ensure_tool_call(output_index, call_id, "");
        self.emit_tool_call_start(position, is_new, output)?;
        let delta = self.merge_full_arguments(position, arguments);
        if !delta.is_empty() {
            self.emit_chat_chunk(
                output,
                json!({
                    "tool_calls": [{
                        "index": position,
                        "function": {"arguments": delta}
                    }]
                }),
                None,
                None,
            )?;
        }
        Ok(())
    }

    fn emit_tool_call_start(
        &mut self,
        position: usize,
        is_new: bool,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        if !is_new {
            return Ok(());
        }
        let call_id = self.tool_calls[position].id.clone();
        let tool_name = self.tool_calls[position].name.clone();
        self.ensure_created(output)?;
        self.emit_chat_chunk(
            output,
            json!({
                "tool_calls": [{
                    "index": position,
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": tool_name,
                        "arguments": ""
                    }
                }]
            }),
            None,
            None,
        )?;
        Ok(())
    }

    fn merge_full_arguments(&mut self, position: usize, arguments: &str) -> String {
        if arguments.is_empty() {
            return String::new();
        }
        let tool_call = &mut self.tool_calls[position];
        if tool_call.arguments.is_empty()
            || (arguments.len() >= tool_call.arguments.len()
                && arguments.starts_with(&tool_call.arguments))
        {
            tool_call.arguments = arguments.to_string();
        } else {
            return String::new();
        }
        let emitted_arguments = tool_call.emitted_arguments.min(tool_call.arguments.len());
        let delta = tool_call.arguments[emitted_arguments..].to_string();
        tool_call.emitted_arguments = tool_call.arguments.len();
        delta
    }

    fn ensure_tool_call(
        &mut self,
        output_index: usize,
        call_id: Option<String>,
        name: &str,
    ) -> (usize, bool) {
        if let Some(position) = self.tool_positions.get(&output_index).copied() {
            if !name.is_empty() {
                self.tool_calls[position].name = name.to_string();
            }
            return (position, false);
        }
        if let Some(call_id) = call_id.as_deref()
            && let Some(position) = self.tool_calls.iter().position(|call| call.id == call_id)
        {
            self.tool_positions.insert(output_index, position);
            return (position, false);
        }
        let position = self.tool_calls.len();
        self.tool_calls.push(ToolCall {
            id: call_id.unwrap_or_else(|| format!("call_{position}")),
            name: name.to_string(),
            arguments: String::new(),
            emitted_arguments: 0,
        });
        self.tool_positions.insert(output_index, position);
        (position, true)
    }

    pub(super) fn observe_completed_response(
        &mut self,
        response: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        self.observe_metadata(response);
        let Some(items) = response.get("output").and_then(Value::as_array) else {
            return Ok(());
        };
        for item in items {
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let output_index = item
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.strip_prefix("output_"))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(self.tool_calls.len());
                self.observe_output_item(
                    &json!({"output_index": output_index, "item": item}),
                    output,
                )?;
            } else if self.full_text.is_empty()
                && item.get("type").and_then(Value::as_str) == Some("message")
            {
                let text = item.get("content").map(extract_text).unwrap_or_default();
                if !text.is_empty() {
                    self.emit_text_delta(&text, output)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn emit_error(
        &mut self,
        value: &Value,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let error = value.get("error").unwrap_or(value);
        let mut error_object = Map::new();
        error_object.insert(
            "message".to_string(),
            error
                .get("message")
                .cloned()
                .unwrap_or_else(|| Value::String("responses upstream failed".to_string())),
        );
        if let Some(error_type) = error.get("type").or_else(|| value.get("type")) {
            error_object.insert("type".to_string(), error_type.clone());
        }
        if let Some(code) = error.get("code") {
            error_object.insert("code".to_string(), code.clone());
        }
        output.push(sse_event(&json!({"error": Value::Object(error_object)}))?);
        Ok(())
    }

    pub(super) fn emit_completion(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        if self.completed {
            return Ok(());
        }
        self.ensure_created(output)?;
        let finish_reason = self.finish_reason.unwrap_or(if self.tool_calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        });
        self.emit_chat_chunk(
            output,
            Value::Object(Map::new()),
            Some(finish_reason),
            self.usage.as_ref().map(responses_usage_to_chat),
        )?;
        output.push(b"data: [DONE]\n\n".to_vec());
        self.completed = true;
        Ok(())
    }

    fn emit_chat_chunk(
        &mut self,
        output: &mut Vec<Vec<u8>>,
        delta: Value,
        finish_reason: Option<&str>,
        usage: Option<Value>,
    ) -> Result<(), CompatError> {
        let mut chunk = json!({
            "id": self.response_id.clone().unwrap_or_else(|| "chatcmpl_compat".to_string()),
            "object": "chat.completion.chunk",
            "created": self.created_at.unwrap_or_else(|| chrono::Utc::now().timestamp()),
            "model": self.model.clone().unwrap_or_else(|| "unknown".to_string()),
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason,
            }]
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        output.push(sse_event(&chunk)?);
        Ok(())
    }
}

fn responses_usage_to_chat(value: &Value) -> Value {
    let Some(usage) = value.as_object() else {
        return Value::Null;
    };
    let mut translated = Map::new();
    if let Some(value) = usage.get("input_tokens") {
        translated.insert("prompt_tokens".to_string(), value.clone());
    }
    if let Some(value) = usage.get("output_tokens") {
        translated.insert("completion_tokens".to_string(), value.clone());
    }
    if let Some(value) = usage.get("total_tokens") {
        translated.insert("total_tokens".to_string(), value.clone());
    }
    if let Some(value) = usage.get("input_tokens_details") {
        translated.insert("prompt_tokens_details".to_string(), value.clone());
    }
    if let Some(value) = usage.get("output_tokens_details") {
        translated.insert("completion_tokens_details".to_string(), value.clone());
    }
    Value::Object(translated)
}
