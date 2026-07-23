use http::StatusCode;
use serde_json::{Map, Value, json};

use super::CompatError;

pub(crate) fn translate_tools(value: &Value) -> Result<Value, CompatError> {
    let tools = value.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "tools must be an array for chat-native endpoints",
        )
    })?;
    Ok(Value::Array(
        tools
            .iter()
            .map(translate_tool)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

pub(crate) fn translate_tool_choice(value: &Value) -> Result<Value, CompatError> {
    match value {
        Value::String(mode) if matches!(mode.as_str(), "auto" | "required" | "none") => {
            Ok(Value::String(mode.clone()))
        }
        Value::Object(object) => translate_named_tool_choice(object),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "tool_choice must be `auto`, `required`, `none`, or a named function",
        )),
    }
}

fn translate_tool(value: &Value) -> Result<Value, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "tool definitions must be JSON objects",
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
            format!("responses tool type `{tool_type}` is not supported for chat-native endpoints"),
        ));
    }

    let source = object
        .get("function")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let name = source
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "function tools require name",
            )
        })?;

    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(name.to_string()));
    for field in ["description", "parameters", "strict"] {
        if let Some(field_value) = source.get(field) {
            function.insert(field.to_string(), field_value.clone());
        }
    }

    Ok(json!({
        "type": "function",
        "function": Value::Object(function),
    }))
}

fn translate_named_tool_choice(object: &Map<String, Value>) -> Result<Value, CompatError> {
    let choice_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function");
    if choice_type != "function" {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "responses tool_choice type `{choice_type}` is not supported for chat-native endpoints"
            ),
        ));
    }
    let name = object
        .get("name")
        .or_else(|| {
            object
                .get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "named tool_choice requires a function name",
            )
        })?;
    Ok(json!({
        "type": "function",
        "function": {
            "name": name,
        }
    }))
}
