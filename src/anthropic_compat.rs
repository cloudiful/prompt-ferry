use http::StatusCode;
use serde_json::{Map, Value, json};

use crate::openai_compat::{CompatError, responses_request_to_chat};

pub fn responses_request_to_anthropic_messages(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let chat_request =
        serde_json::from_slice::<Value>(&responses_request_to_chat(body)?).map_err(|_| {
            CompatError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "adapter_error",
                "failed to decode translated chat request",
            )
        })?;
    let chat_object = chat_request.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "translated chat request must be a JSON object",
        )
    })?;
    let mut anthropic = Map::new();
    anthropic.insert(
        "model".to_string(),
        chat_object.get("model").cloned().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "responses model is required for anthropic-native endpoints",
            )
        })?,
    );
    anthropic.insert(
        "max_tokens".to_string(),
        chat_object
            .get("max_tokens")
            .cloned()
            .unwrap_or(Value::from(1024)),
    );
    if let Some(stream) = chat_object.get("stream").cloned() {
        anthropic.insert("stream".to_string(), stream);
    }
    if let Some(system) = chat_object
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.first())
        .and_then(Value::as_object)
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .and_then(|message| message.get("content"))
        .map(extract_system_text)
        .transpose()?
        .filter(|text| !text.is_empty())
    {
        anthropic.insert("system".to_string(), Value::String(system));
    }
    if let Some(tools) = chat_object
        .get("tools")
        .filter(|value| has_meaningful_value(value))
    {
        anthropic.insert("tools".to_string(), translate_tools(tools)?);
    }
    if let Some(tool_choice) = chat_object
        .get("tool_choice")
        .filter(|value| has_meaningful_value(value))
    {
        anthropic.insert(
            "tool_choice".to_string(),
            translate_tool_choice(tool_choice)?,
        );
    }
    let messages = chat_object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "responses request did not produce chat-compatible messages",
            )
        })?
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
        .cloned()
        .map(translate_message)
        .collect::<Result<Vec<_>, _>>()?;
    if messages.is_empty() {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "responses input must contain at least one supported message",
        ));
    }
    anthropic.insert("messages".to_string(), Value::Array(messages));

    serde_json::to_vec(&Value::Object(anthropic)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated anthropic messages request",
        )
    })
}

pub fn anthropic_response_to_responses(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "anthropic-native endpoint returned invalid JSON",
        )
    })?;
    let response = anthropic_value_to_responses_value(&value)?;
    serde_json::to_vec(&response).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated responses payload",
        )
    })
}

pub fn anthropic_value_to_responses_value(value: &Value) -> Result<Value, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "anthropic-native endpoint returned invalid JSON object",
        )
    })?;
    let id = object.get("id").and_then(Value::as_str).unwrap_or("");
    let model = object.get("model").and_then(Value::as_str);
    let output = anthropic_content_to_output_items(object.get("content"))?;
    let usage = anthropic_usage_to_openai_usage(value);
    Ok(json!({
        "id": if id.is_empty() { generate_id("resp") } else { id.to_string() },
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "completed_at": chrono::Utc::now().timestamp(),
        "error": Value::Null,
        "incomplete_details": Value::Null,
        "metadata": {},
        "model": model.unwrap_or("unknown"),
        "output": output,
        "output_text": output_text_from_items(object.get("content")),
        "parallel_tool_calls": false,
        "status": "completed",
        "store": false,
        "text": { "format": { "type": "text" } },
        "tool_choice": "auto",
        "truncation": "disabled",
        "usage": usage,
    }))
}

fn translate_message(message: Value) -> Result<Value, CompatError> {
    let object = message.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "anthropic messages require JSON object messages",
        )
    })?;
    let role = object.get("role").and_then(Value::as_str).unwrap_or("user");
    let content = if role == "assistant" {
        translate_assistant_content(object)?
    } else if role == "tool" {
        translate_tool_result_content(object)?
    } else {
        translate_user_content(object.get("content").unwrap_or(&Value::Null))?
    };
    Ok(json!({
        "role": if role == "tool" { "user" } else { role },
        "content": content,
    }))
}

fn translate_user_content(content: &Value) -> Result<Value, CompatError> {
    match content {
        Value::String(text) => Ok(Value::String(text.to_string())),
        Value::Array(parts) => Ok(Value::Array(
            parts
                .iter()
                .filter_map(|part| {
                    let part_object = part.as_object()?;
                    let part_type = part_object.get("type").and_then(Value::as_str)?;
                    match part_type {
                        "text" | "input_text" => Some(json!({
                            "type": "text",
                            "text": part_object.get("text").and_then(Value::as_str).unwrap_or_default(),
                        })),
                        _ => None,
                    }
                })
                .collect(),
        )),
        Value::Null => Ok(Value::String(String::new())),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "anthropic messages support only text content in this compatibility mode",
        )),
    }
}

fn translate_assistant_content(object: &Map<String, Value>) -> Result<Value, CompatError> {
    let mut parts = Vec::new();
    if let Some(reasoning) = object
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(json!({
            "type": "thinking",
            "thinking": reasoning,
        }));
    }
    if let Some(content) = object.get("content") {
        match content {
            Value::String(text) if !text.is_empty() => parts.push(json!({
                "type": "text",
                "text": text,
            })),
            Value::Array(items) => {
                for item in items {
                    let Some(part) = translate_text_part(item) else {
                        continue;
                    };
                    parts.push(part);
                }
            }
            _ => {}
        }
    }
    if let Some(tool_calls) = object.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let tool_object = tool_call.as_object().ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "tool_calls entries must be JSON objects",
                )
            })?;
            let function = tool_object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "tool calls require function details",
                    )
                })?;
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            parts.push(json!({
                "type": "tool_use",
                "id": tool_object.get("id").and_then(Value::as_str).unwrap_or_default(),
                "name": function.get("name").and_then(Value::as_str).unwrap_or_default(),
                "input": serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({})),
            }));
        }
    }
    Ok(Value::Array(parts))
}

fn translate_tool_result_content(object: &Map<String, Value>) -> Result<Value, CompatError> {
    let tool_use_id = object
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "tool role messages require tool_call_id for anthropic-native endpoints",
            )
        })?;
    let text = match object.get("content") {
        Some(Value::String(text)) => text.to_string(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>(),
        Some(Value::Null) | None => String::new(),
        _ => {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "tool result content must be text for anthropic-native endpoints",
            ));
        }
    };
    Ok(Value::Array(vec![json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": text,
    })]))
}

fn translate_tools(value: &Value) -> Result<Value, CompatError> {
    let tools = value.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses tools must be an array for anthropic-native endpoints",
        )
    })?;
    Ok(Value::Array(
        tools
            .iter()
            .map(|tool| {
                let object = tool.as_object().ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "responses tools entries must be JSON objects",
                    )
                })?;
                let tool_type = object.get("type").and_then(Value::as_str).unwrap_or("function");
                if tool_type != "function" {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        format!(
                            "responses tool type `{tool_type}` is not supported for anthropic-native endpoints"
                        ),
                    ));
                }
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .unwrap_or(object);
                Ok(json!({
                    "name": function.get("name").and_then(Value::as_str).unwrap_or_default(),
                    "description": function.get("description").and_then(Value::as_str).unwrap_or_default(),
                    "input_schema": function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({"type":"object","properties":{}})),
                }))
            })
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn translate_tool_choice(value: &Value) -> Result<Value, CompatError> {
    match value {
        Value::String(choice) => Ok(match choice.as_str() {
            "auto" => json!({"type":"auto"}),
            "required" => json!({"type":"any"}),
            "none" => json!({"type":"none"}),
            other => {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    format!(
                        "responses tool_choice `{other}` is not supported for anthropic-native endpoints"
                    ),
                ));
            }
        }),
        Value::Object(object) => {
            let choice_type = object.get("type").and_then(Value::as_str).unwrap_or("auto");
            if choice_type == "function" {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .unwrap_or(object);
                Ok(json!({
                    "type": "tool",
                    "name": function.get("name").and_then(Value::as_str).unwrap_or_default(),
                }))
            } else {
                translate_tool_choice(&Value::String(choice_type.to_string()))
            }
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses tool_choice must be a string or object for anthropic-native endpoints",
        )),
    }
}

fn anthropic_content_to_output_items(content: Option<&Value>) -> Result<Vec<Value>, CompatError> {
    let mut output = Vec::new();
    let Some(items) = content.and_then(Value::as_array) else {
        return Ok(output);
    };
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        match object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "thinking" => output.push(json!({
                "id": generate_id("rsn"),
                "type": "reasoning",
                "status": "completed",
                "summary": [],
                "content": [{
                    "type": "reasoning_text",
                    "text": object.get("thinking").and_then(Value::as_str).unwrap_or_default(),
                }],
            })),
            "text" => output.push(json!({
                "id": generate_id("msg"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": object.get("text").and_then(Value::as_str).unwrap_or_default(),
                    "annotations": [],
                    "logprobs": [],
                }],
            })),
            "tool_use" => output.push(json!({
                "id": object.get("id").and_then(Value::as_str).unwrap_or_default(),
                "type": "function_call",
                "status": "completed",
                "call_id": object.get("id").and_then(Value::as_str).unwrap_or_default(),
                "name": object.get("name").and_then(Value::as_str).unwrap_or_default(),
                "arguments": serde_json::to_string(object.get("input").unwrap_or(&json!({})))
                    .unwrap_or_else(|_| "{}".to_string()),
            })),
            _ => {}
        }
    }
    Ok(output)
}

fn anthropic_usage_to_openai_usage(value: &Value) -> Option<Value> {
    if let Some(usage) = value.get("usage").and_then(Value::as_object) {
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
        return Some(json!({
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
        }));
    }
    None
}

fn output_text_from_items(content: Option<&Value>) -> String {
    content
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let object = item.as_object()?;
                    (object.get("type").and_then(Value::as_str) == Some("text")).then(|| {
                        object
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    })
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn translate_text_part(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let part_type = object.get("type").and_then(Value::as_str)?;
    match part_type {
        "text" | "input_text" | "output_text" => Some(json!({
            "type": "text",
            "text": object.get("text").and_then(Value::as_str).unwrap_or_default(),
        })),
        _ => None,
    }
}

fn extract_system_text(value: &Value) -> Result<String, CompatError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => Ok(parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>()),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "system messages must be text for anthropic-native endpoints",
        )),
    }
}

fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}

fn generate_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{anthropic_response_to_responses, responses_request_to_anthropic_messages};

    #[test]
    fn translates_responses_request_to_anthropic_messages() {
        let body = br#"{
            "model":"claude-sonnet-4-5",
            "input":"Hello",
            "max_output_tokens":4,
            "tools":[{"type":"function","name":"lookup","description":"Lookup weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}],
            "tool_choice":"auto"
        }"#;
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_anthropic_messages(body).unwrap(),
        )
        .unwrap();
        assert_eq!(value["model"].as_str(), Some("claude-sonnet-4-5"));
        assert_eq!(value["max_tokens"].as_i64(), Some(4));
        assert_eq!(value["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(value["messages"][0]["content"].as_str(), Some("Hello"));
        assert_eq!(value["tools"][0]["name"].as_str(), Some("lookup"));
    }

    #[test]
    fn translates_function_tool_choice_to_anthropic_tool_choice() {
        let body = br#"{
            "model":"claude-sonnet-4-5",
            "input":"Hello",
            "tools":[{"type":"function","name":"lookup","description":"Lookup weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}],
            "tool_choice":{"type":"function","name":"lookup"}
        }"#;
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_anthropic_messages(body).unwrap(),
        )
        .unwrap();
        assert_eq!(value["tool_choice"]["type"].as_str(), Some("tool"));
        assert_eq!(value["tool_choice"]["name"].as_str(), Some("lookup"));
    }

    #[test]
    fn translates_anthropic_response_to_responses() {
        let body = br#"{
            "id":"msg_123",
            "model":"claude-sonnet-4-5",
            "content":[
                {"type":"text","text":"Hello"},
                {"type":"tool_use","id":"toolu_1","name":"lookup","input":{"city":"Boston"}}
            ],
            "usage":{"input_tokens":2,"output_tokens":3}
        }"#;
        let value =
            serde_json::from_slice::<Value>(&anthropic_response_to_responses(body).unwrap())
                .unwrap();
        assert_eq!(value["object"].as_str(), Some("response"));
        assert_eq!(value["output_text"].as_str(), Some("Hello"));
        assert_eq!(value["output"][1]["type"].as_str(), Some("function_call"));
        assert_eq!(value["usage"]["input_tokens"].as_i64(), Some(2));
        assert_eq!(value["usage"]["output_tokens"].as_i64(), Some(3));
    }

    #[test]
    fn preserves_anthropic_cache_read_and_write_usage() {
        let body = br#"{
            "id":"msg_123",
            "model":"claude-sonnet-4-5",
            "content":[],
            "usage":{
                "input_tokens":5,
                "cache_read_input_tokens":3,
                "cache_creation_input_tokens":4,
                "output_tokens":2
            }
        }"#;
        let value =
            serde_json::from_slice::<Value>(&anthropic_response_to_responses(body).unwrap())
                .unwrap();

        assert_eq!(value["usage"]["input_tokens"].as_i64(), Some(12));
        assert_eq!(
            value["usage"]["input_tokens_details"]["cache_read_tokens"].as_i64(),
            Some(3)
        );
        assert_eq!(
            value["usage"]["input_tokens_details"]["cache_write_tokens"].as_i64(),
            Some(4)
        );
    }

    #[test]
    fn leaves_usage_null_when_anthropic_does_not_report_tokens() {
        let body = br#"{
            "id":"msg_123",
            "model":"claude-sonnet-4-5",
            "content":[],
            "usage":{}
        }"#;
        let value =
            serde_json::from_slice::<Value>(&anthropic_response_to_responses(body).unwrap())
                .unwrap();

        assert!(value["usage"].is_null());
    }
}
