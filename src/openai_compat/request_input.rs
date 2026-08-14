use http::StatusCode;
use serde_json::{Map, Value, json};

mod tool_calls;

use super::{
    CompatError,
    request_content::{translate_assistant_content, translate_content},
};
use tool_calls::{
    is_tool_call_item, required_string_field, translate_function_call_message,
    translate_function_call_output,
};

pub(crate) fn translate_input(input: &Value) -> Result<Vec<Value>, CompatError> {
    match input {
        Value::String(text) => Ok(vec![json!({
            "role": "user",
            "content": text,
        })]),
        Value::Array(items) => {
            tool_calls::translate_items(items, translate_message, translate_reasoning_item)
        }
        value if is_tool_call_item(value) => tool_calls::translate_items(
            std::slice::from_ref(value),
            translate_message,
            translate_reasoning_item,
        ),
        value => translate_message(value).map(|message| message.into_iter().collect()),
    }
}

fn translate_message(item: &Value) -> Result<Option<Value>, CompatError> {
    match item {
        Value::String(text) => Ok(Some(json!({
            "role": "user",
            "content": text,
        }))),
        Value::Object(object) => translate_object_message(item, object),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses input item must be a string, object, or array",
        )),
    }
}

fn translate_object_message(
    item: &Value,
    object: &Map<String, Value>,
) -> Result<Option<Value>, CompatError> {
    if let Some(item_type) = object.get("type").and_then(Value::as_str) {
        match item_type {
            "function_call" => return Ok(Some(translate_function_call_message(object)?)),
            "function_call_output" => {
                return Ok(Some(translate_function_call_output(object)?));
            }
            "reasoning" => return Ok(None),
            _ => {}
        }
    }
    if let Some(role) = object.get("role").and_then(Value::as_str) {
        return translate_role_message(object, role, item);
    }
    if object.get("type").is_some() {
        let content = translate_content(item)?;
        if content_is_empty(&content) {
            return Ok(None);
        }
        return Ok(Some(json!({
            "role": "user",
            "content": content,
        })));
    }
    Err(CompatError::new(
        StatusCode::BAD_REQUEST,
        "unsupported_feature",
        "responses input item must be a role message, function call item, function result item, or supported text/image part",
    ))
}

fn translate_reasoning_item(item: &Value) -> Result<Option<String>, CompatError> {
    let Some(object) = item.as_object() else {
        return Ok(None);
    };
    if object.get("content").is_none() {
        return Ok(None);
    }
    Ok(translate_assistant_content(item)?.reasoning_content)
}

fn translate_role_message(
    object: &Map<String, Value>,
    role: &str,
    original: &Value,
) -> Result<Option<Value>, CompatError> {
    let mut message = Map::new();
    message.insert("role".to_string(), Value::String(role.to_string()));

    if let Some(tool_call_id) = object.get("tool_call_id").and_then(Value::as_str) {
        message.insert(
            "tool_call_id".to_string(),
            Value::String(tool_call_id.to_string()),
        );
    }
    if let Some(tool_calls) = object.get("tool_calls")
        && super::request::has_meaningful_value(tool_calls)
    {
        message.insert(
            "tool_calls".to_string(),
            translate_chat_tool_calls(tool_calls)?,
        );
    }

    let (content, reasoning_content) = if role == "assistant" {
        if let Some(content) = object.get("content") {
            let translated = translate_assistant_content(content)?;
            (Some(translated.content), translated.reasoning_content)
        } else if object.get("type").is_some() {
            let translated = translate_assistant_content(original)?;
            (Some(translated.content), translated.reasoning_content)
        } else if message.contains_key("tool_calls") {
            (Some(Value::Null), None)
        } else {
            (None, None)
        }
    } else if object.get("type").is_some() {
        (Some(translate_content(original)?), None)
    } else if let Some(content) = object.get("content") {
        (Some(translate_content(content)?), None)
    } else {
        (None, None)
    };
    let Some(content) = content else {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses message content is required for chat-native endpoints",
        ));
    };
    let assistant_has_auxiliary =
        role == "assistant" && (message.contains_key("tool_calls") || reasoning_content.is_some());
    if content_is_empty(&content) && !assistant_has_auxiliary {
        return Ok(None);
    }
    message.insert(
        "content".to_string(),
        if role == "assistant" && content_is_empty(&content) {
            Value::Null
        } else {
            content
        },
    );
    if let Some(reasoning_content) = reasoning_content {
        message.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning_content),
        );
    } else if role == "assistant"
        && let Some(reasoning_details) = object
            .get("reasoning_details")
            .filter(|value| super::request::has_meaningful_value(value))
    {
        let reasoning_content = super::extract_text(reasoning_details);
        if !reasoning_content.trim().is_empty() {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(reasoning_content),
            );
        }
    }

    Ok(Some(Value::Object(message)))
}

fn translate_chat_tool_calls(value: &Value) -> Result<Value, CompatError> {
    let tool_calls = value.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "tool_calls must be an array",
        )
    })?;
    Ok(Value::Array(
        tool_calls
            .iter()
            .map(translate_chat_tool_call)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn translate_chat_tool_call(value: &Value) -> Result<Value, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "tool_calls entries must be JSON objects",
        )
    })?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function");
    if tool_type != "function" {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("tool call type `{tool_type}` is not supported for chat-native endpoints"),
        ));
    }
    let id = required_string_field(object, &["id"], "tool calls require id")?;
    let function = object
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "tool calls require function details",
            )
        })?;
    let name = required_string_field(function, &["name"], "tool calls require function name")?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default();

    Ok(json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

fn content_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}
