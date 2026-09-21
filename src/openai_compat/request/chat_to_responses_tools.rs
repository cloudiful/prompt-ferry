use super::*;
use std::collections::{HashMap, HashSet};

/// Chat history can lose a `tool` message (client-side prune or truncation)
/// while the assistant `tool_calls` turn survives. Responses rejects such a
/// request with `No tool output found for function call`, so synthesize the
/// placeholder output right after the orphan call.
pub(super) fn ensure_tool_pairing(input: Vec<Value>) -> (Vec<Value>, Vec<String>) {
    let mut calls: Vec<(usize, String)> = Vec::new();
    let mut outputs: HashSet<String> = HashSet::new();
    for (index, item) in input.iter().enumerate() {
        let Some(object) = item.as_object() else {
            continue;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                if let Some(id) = object.get("call_id").and_then(Value::as_str) {
                    calls.push((index, id.to_string()));
                }
            }
            Some("function_call_output") => {
                if let Some(id) = object.get("call_id").and_then(Value::as_str) {
                    outputs.insert(id.to_string());
                }
            }
            _ => {}
        }
    }
    let missing: Vec<(usize, String)> = calls
        .into_iter()
        .filter(|(_, id)| !outputs.contains(id))
        .collect();
    if missing.is_empty() {
        return (input, Vec::new());
    }
    let insert_at: HashMap<usize, String> = missing.iter().cloned().collect();
    let synthesized: Vec<String> = missing.iter().map(|(_, id)| id.clone()).collect();
    let mut result = Vec::with_capacity(input.len() + synthesized.len());
    for (index, item) in input.into_iter().enumerate() {
        result.push(item);
        if let Some(id) = insert_at.get(&index) {
            result.push(json!({
                "type": "function_call_output",
                "call_id": id,
                "output": format!("[Missing tool output: pruned or truncated, call_id={id}]"),
            }));
        }
    }
    (result, synthesized)
}

pub(super) fn translate_chat_tool_calls(value: Option<&Value>) -> Result<Vec<Value>, CompatError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let calls = value.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat tool_calls must be an array",
        )
    })?;
    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "chat tool calls must be JSON objects",
                )
            })?;
            let call_id = object
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool calls require an id",
                    )
                })?;
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool calls require function details",
                    )
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool calls require a function name",
                    )
                })?;
            Ok(json!({
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": function.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
            }))
        })
        .collect()
}

pub(super) fn translate_chat_tool_output(
    message: &Map<String, Value>,
    content: &Value,
) -> Result<Value, CompatError> {
    let call_id = message
        .get("tool_call_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "chat tool messages require tool_call_id",
            )
        })?;
    Ok(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": chat_tool_output(content)?,
    }))
}

fn chat_tool_output(content: &Value) -> Result<Value, CompatError> {
    match content {
        Value::String(_) | Value::Null => Ok(content.clone()),
        Value::Array(parts) => parts
            .iter()
            .map(|part| {
                let object = part.as_object().ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool output parts must be JSON objects",
                    )
                })?;
                if object.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool output only supports text parts",
                    ));
                }
                object.get("text").cloned().ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat tool output text parts require text",
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat tool output must be text or text parts",
        )),
    }
}

pub(super) fn translate_chat_tools(value: &Value) -> Result<Value, CompatError> {
    let tools = value.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat tools must be an array",
        )
    })?;
    tools
        .iter()
        .map(|tool| {
            let object = tool.as_object().ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "chat tool definitions must be JSON objects",
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "only function tools are supported for Responses compatibility",
                ));
            }
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat function tools require function details",
                    )
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat function tools require a name",
                    )
                })?;
            let mut translated = Map::new();
            translated.insert("type".to_string(), Value::String("function".to_string()));
            translated.insert("name".to_string(), Value::String(name.to_string()));
            for field in ["description", "parameters", "strict"] {
                if let Some(value) = function.get(field) {
                    translated.insert(field.to_string(), value.clone());
                }
            }
            Ok(Value::Object(translated))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

pub(super) fn translate_chat_tool_choice(value: &Value) -> Result<Value, CompatError> {
    match value {
        Value::String(mode) if matches!(mode.as_str(), "auto" | "none" | "required") => {
            Ok(value.clone())
        }
        Value::Object(object) => {
            let name = object
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .or_else(|| object.get("name").and_then(Value::as_str))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "named chat tool_choice requires a function name",
                    )
                })?;
            Ok(json!({ "type": "function", "name": name }))
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat tool_choice must be auto, none, required, or a named function",
        )),
    }
}
