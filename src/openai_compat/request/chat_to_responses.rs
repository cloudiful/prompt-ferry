use super::*;
use serde_json::json;

#[path = "chat_to_responses_content.rs"]
mod chat_to_responses_content;
#[path = "chat_to_responses_tools.rs"]
mod chat_to_responses_tools;

use chat_to_responses_content::{chat_content_to_responses_parts, chat_content_to_text};
use chat_to_responses_tools::{
    translate_chat_tool_calls, translate_chat_tool_choice, translate_chat_tool_output,
    translate_chat_tools,
};

pub fn chat_request_to_responses(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let object = parse_chat_request_object(body)?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "chat request must contain a messages array",
            )
        })?;

    let mut instructions = Vec::new();
    let mut input = Vec::new();
    for message in messages {
        let message_object = message.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "chat messages must be JSON objects",
            )
        })?;
        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "chat messages require a role",
                )
            })?;
        let content = message_object.get("content").unwrap_or(&Value::Null);
        match role {
            "system" | "developer" => {
                let text = chat_content_to_text(content)?;
                if !text.trim().is_empty() {
                    instructions.push(text);
                }
            }
            "user" | "assistant" => {
                let parts = chat_content_to_responses_parts(content, role == "assistant")?;
                if !parts.is_empty() {
                    input.push(json!({
                        "role": role,
                        "content": parts,
                    }));
                } else if role == "assistant"
                    && message_object
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|calls| !calls.is_empty())
                {
                    input.push(json!({
                        "role": "assistant",
                        "content": Value::Null,
                    }));
                }
                if role == "assistant" {
                    input.extend(translate_chat_tool_calls(message_object.get("tool_calls"))?);
                }
            }
            "tool" => input.push(translate_chat_tool_output(message_object, content)?),
            other => {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    format!("chat message role `{other}` is not supported for Responses"),
                ));
            }
        }
    }
    if input.is_empty() && instructions.is_empty() {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "chat request must contain at least one message",
        ));
    }

    let mut responses = Map::new();
    for field in [
        "model",
        "stream",
        "temperature",
        "top_p",
        "parallel_tool_calls",
        "store",
        "metadata",
        "prompt_cache_key",
        "service_tier",
    ] {
        if let Some(value) = object.get(field) {
            responses.insert(field.to_string(), value.clone());
        }
    }
    if let Some(value) = object
        .get("max_completion_tokens")
        .or_else(|| object.get("max_tokens"))
    {
        responses.insert("max_output_tokens".to_string(), value.clone());
    }
    if let Some(value) = object
        .get("reasoning_effort")
        .filter(|value| has_meaningful_value(value))
    {
        responses.insert("reasoning".to_string(), json!({ "effort": value }));
    }
    if let Some(value) = object.get("response_format") {
        responses.insert(
            "text".to_string(),
            json!({ "format": chat_response_format(value)? }),
        );
    }
    if let Some(value) = object
        .get("tools")
        .filter(|value| has_meaningful_value(value))
    {
        responses.insert("tools".to_string(), translate_chat_tools(value)?);
    }
    if let Some(value) = object
        .get("tool_choice")
        .filter(|value| has_meaningful_value(value))
    {
        responses.insert(
            "tool_choice".to_string(),
            translate_chat_tool_choice(value)?,
        );
    }
    if !instructions.is_empty() {
        responses.insert(
            "instructions".to_string(),
            Value::String(instructions.join("\n\n")),
        );
    }
    responses.insert("input".to_string(), Value::Array(input));

    serde_json::to_vec(&Value::Object(responses)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated Responses request",
        )
    })
}

fn parse_chat_request_object(body: &[u8]) -> Result<Map<String, Value>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "request body must be valid JSON",
        )
    })?;
    value.as_object().cloned().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "chat request must be a JSON object",
        )
    })
}

fn chat_response_format(value: &Value) -> Result<Value, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat response_format must be an object",
        )
    })?;
    match object.get("type").and_then(Value::as_str).unwrap_or("text") {
        "text" | "json_object" => Ok(json!({
            "type": object.get("type").and_then(Value::as_str).unwrap_or("text"),
        })),
        "json_schema" => {
            let schema = object
                .get("json_schema")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat json_schema response_format requires json_schema",
                    )
                })?;
            let mut format = Map::new();
            format.insert("type".to_string(), Value::String("json_schema".to_string()));
            for field in ["name", "description", "schema", "strict"] {
                if let Some(value) = schema.get(field) {
                    format.insert(field.to_string(), value.clone());
                }
            }
            Ok(Value::Object(format))
        }
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("chat response_format type `{other}` is not supported for Responses"),
        )),
    }
}
