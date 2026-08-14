use http::StatusCode;
use serde_json::{Map, Value, json};

use super::super::{CompatError, request_content::translate_tool_output_content};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCallItemKind {
    FunctionCall,
    FunctionCallOutput,
}

pub(super) struct ToolCallTurn {
    function_calls: Vec<Value>,
    outputs: Vec<Value>,
    reasoning_content: String,
}

impl ToolCallTurn {
    fn new() -> Self {
        Self {
            function_calls: Vec::new(),
            outputs: Vec::new(),
            reasoning_content: String::new(),
        }
    }

    fn push_reasoning(&mut self, text: &str) {
        self.reasoning_content.push_str(text);
    }

    fn push(&mut self, item: &Value) -> Result<(), CompatError> {
        let object = item.as_object().expect("tool call item must be an object");
        match tool_call_item_kind(item) {
            Some(ToolCallItemKind::FunctionCall) => {
                self.function_calls.push(translate_function_call(object)?);
            }
            Some(ToolCallItemKind::FunctionCallOutput) => {
                self.outputs.push(translate_function_call_output(object)?);
            }
            None => unreachable!("non-tool item passed to tool call turn"),
        }
        Ok(())
    }

    fn has_outputs(&self) -> bool {
        !self.outputs.is_empty()
    }

    fn finish(self) -> Vec<Value> {
        let mut messages =
            Vec::with_capacity(usize::from(!self.function_calls.is_empty()) + self.outputs.len());
        if !self.function_calls.is_empty() {
            let mut assistant = json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": self.function_calls,
            });
            if !self.reasoning_content.trim().is_empty()
                && let Some(object) = assistant.as_object_mut()
            {
                object.insert(
                    "reasoning_content".to_string(),
                    Value::String(self.reasoning_content),
                );
            }
            messages.push(assistant);
        }
        messages.extend(self.outputs);
        messages
    }
}

pub(super) fn translate_items<F, R>(
    items: &[Value],
    mut translate_regular_item: F,
    mut translate_reasoning_item: R,
) -> Result<Vec<Value>, CompatError>
where
    F: FnMut(&Value) -> Result<Option<Value>, CompatError>,
    R: FnMut(&Value) -> Result<Option<String>, CompatError>,
{
    let mut messages = Vec::new();
    let mut turn = ToolCallTurn::new();
    for item in items {
        if is_tool_call_item(item) {
            if matches!(
                tool_call_item_kind(item),
                Some(ToolCallItemKind::FunctionCall)
            ) && turn.has_outputs()
            {
                messages.extend(turn.finish());
                turn = ToolCallTurn::new();
            }
            turn.push(item)?;
            continue;
        }
        if item.as_object().is_some_and(|object| {
            object.get("role").is_none()
                && object.get("type").and_then(Value::as_str) == Some("reasoning")
        }) {
            if turn.has_outputs() {
                messages.extend(turn.finish());
                turn = ToolCallTurn::new();
            }
            if let Some(reasoning) = translate_reasoning_item(item)? {
                turn.push_reasoning(&reasoning);
            }
            continue;
        }
        messages.extend(turn.finish());
        turn = ToolCallTurn::new();
        if let Some(message) = translate_regular_item(item)? {
            messages.push(message);
        }
    }
    messages.extend(turn.finish());
    Ok(messages)
}

pub(super) fn is_tool_call_item(value: &Value) -> bool {
    tool_call_item_kind(value).is_some()
}

pub(super) fn translate_function_call(object: &Map<String, Value>) -> Result<Value, CompatError> {
    let call_id = required_string_field(
        object,
        &["call_id", "id"],
        "function_call items require call_id",
    )?;
    let name = required_string_field(object, &["name"], "function_call items require name")?;
    let arguments = object
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default();

    Ok(json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

pub(super) fn translate_function_call_message(
    object: &Map<String, Value>,
) -> Result<Value, CompatError> {
    Ok(json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": [translate_function_call(object)?],
    }))
}

pub(super) fn translate_function_call_output(
    object: &Map<String, Value>,
) -> Result<Value, CompatError> {
    let call_id = required_string_field(
        object,
        &["call_id"],
        "function_call_output items require call_id",
    )?;
    let output = object.get("output").ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "function_call_output items require output",
        )
    })?;

    Ok(json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": translate_tool_output_content(output)?,
    }))
}

pub(super) fn required_string_field<'a>(
    object: &'a Map<String, Value>,
    keys: &[&str],
    message: &'static str,
) -> Result<&'a str, CompatError> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CompatError::new(StatusCode::BAD_REQUEST, "unsupported_feature", message))
}

fn tool_call_item_kind(value: &Value) -> Option<ToolCallItemKind> {
    match value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
    {
        Some("function_call") => Some(ToolCallItemKind::FunctionCall),
        Some("function_call_output") => Some(ToolCallItemKind::FunctionCallOutput),
        _ => None,
    }
}
