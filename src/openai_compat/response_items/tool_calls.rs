use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ChatToolCallDelta {
    pub index: usize,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub(super) struct FunctionToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

pub(crate) fn extract_chat_delta_tool_calls(
    value: &Value,
) -> Result<Vec<ChatToolCallDelta>, CompatError> {
    let mut calls = Vec::new();
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return Ok(calls);
    };
    for choice in choices {
        let Some(delta) = choice.get("delta") else {
            continue;
        };
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, tool_call) in tool_calls.iter().enumerate() {
                calls.push(parse_chat_tool_call_delta(tool_call, position)?);
            }
            continue;
        }
        if let Some(function_call) = delta.get("function_call") {
            calls.push(parse_legacy_function_call_delta(function_call)?);
        }
    }
    Ok(calls)
}

fn parse_chat_tool_call(value: &Value) -> Result<FunctionToolCall, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "chat-native endpoint returned an invalid tool call object",
        )
    })?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function");
    if tool_type != "function" {
        return Err(CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            format!("chat-native endpoint returned unsupported tool call type `{tool_type}`"),
        ));
    }
    let function = object
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "chat-native endpoint returned a tool call without function details",
            )
        })?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let call_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(generate_call_id);

    Ok(FunctionToolCall {
        call_id,
        name,
        arguments,
    })
}

fn parse_legacy_function_call(value: &Value) -> Result<FunctionToolCall, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "chat-native endpoint returned an invalid legacy function call object",
        )
    })?;
    Ok(FunctionToolCall {
        call_id: generate_call_id(),
        name: object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: object
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn parse_chat_tool_call_delta(
    value: &Value,
    fallback_index: usize,
) -> Result<ChatToolCallDelta, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "chat-native endpoint returned an invalid streaming tool call object",
        )
    })?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function");
    if tool_type != "function" {
        return Err(CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            format!(
                "chat-native endpoint returned unsupported streaming tool call type `{tool_type}`"
            ),
        ));
    }
    let function = object.get("function").and_then(Value::as_object);
    Ok(ChatToolCallDelta {
        index: object
            .get("index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(fallback_index),
        call_id: object
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        name: function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        arguments: function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn parse_legacy_function_call_delta(value: &Value) -> Result<ChatToolCallDelta, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "chat-native endpoint returned an invalid legacy function-call delta",
        )
    })?;
    Ok(ChatToolCallDelta {
        index: 0,
        call_id: None,
        name: object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        arguments: object
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

pub(super) fn chat_tool_calls_from_message(
    message: &Value,
) -> Result<Vec<FunctionToolCall>, CompatError> {
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        return tool_calls
            .iter()
            .map(parse_chat_tool_call)
            .collect::<Result<Vec<_>, _>>();
    }
    if let Some(function_call) = message.get("function_call") {
        return Ok(vec![parse_legacy_function_call(function_call)?]);
    }
    Ok(Vec::new())
}
