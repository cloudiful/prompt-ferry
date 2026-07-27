use http::StatusCode;
use serde_json::{Map, Value, json};

use super::{
    CompatError,
    response_items::{
        build_response_object, chat_output_items_from_response, usage_from_chat_value,
    },
};

pub fn chat_response_to_responses(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "chat-native endpoint returned invalid JSON",
        )
    })?;
    let response = build_response_object(
        value.get("id").and_then(Value::as_str).unwrap_or(""),
        value.get("model").and_then(Value::as_str),
        value.get("created").and_then(Value::as_i64),
        chat_output_items_from_response(&value)?,
        usage_from_chat_value(&value),
        "completed",
    );
    serde_json::to_vec(&response).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated responses payload",
        )
    })
}

pub fn responses_response_to_chat(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "responses-native endpoint returned invalid JSON",
        )
    })?;
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for item in &output {
        let Some(object) = item.as_object() else {
            return Err(CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "responses-native endpoint returned an invalid output item",
            ));
        };
        match object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message")
        {
            "message" => content.push_str(
                &object
                    .get("content")
                    .map(super::response_items::extract_text)
                    .unwrap_or_default(),
            ),
            "reasoning" => reasoning.push_str(
                &object
                    .get("content")
                    .or_else(|| object.get("summary"))
                    .map(super::response_items::extract_text)
                    .unwrap_or_default(),
            ),
            "function_call" => {
                let call_id = object
                    .get("call_id")
                    .or_else(|| object.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                tool_calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": object.get("name").and_then(Value::as_str).unwrap_or_default(),
                        "arguments": object.get("arguments").and_then(Value::as_str).unwrap_or_default(),
                    }
                }));
            }
            _ => {}
        }
    }
    if content.is_empty() {
        content = value
            .get("output_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert(
        "content".to_string(),
        if content.is_empty() {
            Value::Null
        } else {
            Value::String(content)
        },
    );
    if !reasoning.trim().is_empty() {
        message.insert("reasoning_content".to_string(), Value::String(reasoning));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls.clone()));
    }

    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let finish_reason = if !tool_calls.is_empty() {
        "tool_calls"
    } else if status == "incomplete" {
        "length"
    } else {
        "stop"
    };
    let mut response = Map::new();
    response.insert(
        "id".to_string(),
        value
            .get("id")
            .cloned()
            .unwrap_or_else(|| Value::String("".to_string())),
    );
    response.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    response.insert(
        "created".to_string(),
        value
            .get("created_at")
            .or_else(|| value.get("created"))
            .cloned()
            .unwrap_or_else(|| Value::from(chrono::Utc::now().timestamp())),
    );
    response.insert(
        "model".to_string(),
        value
            .get("model")
            .cloned()
            .unwrap_or_else(|| Value::String("unknown".to_string())),
    );
    response.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
            "logprobs": Value::Null,
        })]),
    );
    response.insert(
        "usage".to_string(),
        responses_usage_to_chat(value.get("usage")),
    );
    if let Some(fingerprint) = value.get("system_fingerprint") {
        response.insert("system_fingerprint".to_string(), fingerprint.clone());
    }

    serde_json::to_vec(&Value::Object(response)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated chat response",
        )
    })
}

fn responses_usage_to_chat(value: Option<&Value>) -> Value {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Value::Null;
    };
    let input_tokens = usage.get("input_tokens").and_then(Value::as_i64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_i64);
    let total_tokens = usage.get("total_tokens").and_then(Value::as_i64);
    if input_tokens.is_none() && output_tokens.is_none() && total_tokens.is_none() {
        return Value::Null;
    }
    let mut translated = Map::new();
    if let Some(value) = input_tokens {
        translated.insert("prompt_tokens".to_string(), Value::from(value));
    }
    if let Some(value) = output_tokens {
        translated.insert("completion_tokens".to_string(), Value::from(value));
    }
    if let Some(value) = total_tokens {
        translated.insert("total_tokens".to_string(), Value::from(value));
    }
    if let Some(cached_tokens) = usage
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
    {
        translated.insert(
            "prompt_tokens_details".to_string(),
            json!({ "cached_tokens": cached_tokens }),
        );
    }
    if let Some(reasoning_tokens) = usage
        .get("output_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64)
    {
        translated.insert(
            "completion_tokens_details".to_string(),
            json!({ "reasoning_tokens": reasoning_tokens }),
        );
    }
    Value::Object(translated)
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use serde_json::Value;

    use super::chat_response_to_responses;

    #[test]
    fn translates_chat_json_response() {
        let body = br#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"gpt-test",
            "choices":[{"message":{"content":"hello"}}],
            "usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}
        }"#;
        let value =
            serde_json::from_slice::<Value>(&chat_response_to_responses(body).unwrap()).unwrap();

        assert_eq!(
            value.get("object").and_then(Value::as_str),
            Some("response")
        );
        assert_eq!(
            value.get("output_text").and_then(Value::as_str),
            Some("hello")
        );
        assert_eq!(value["usage"]["input_tokens"].as_i64(), Some(2));
        assert_eq!(value["text"]["format"]["type"].as_str(), Some("text"));
        assert_eq!(value["truncation"].as_str(), Some("disabled"));
        assert_eq!(
            value["usage"]["output_tokens_details"]["reasoning_tokens"].as_i64(),
            Some(0)
        );
    }

    #[test]
    fn translates_chat_json_response_with_reasoning() {
        let body = br#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"deepseek-test",
            "choices":[{"message":{"reasoning_content":"internal steps","content":"hello"}}]
        }"#;
        let value =
            serde_json::from_slice::<Value>(&chat_response_to_responses(body).unwrap()).unwrap();

        assert_eq!(value["output"][0]["type"].as_str(), Some("reasoning"));
        assert_eq!(
            value["output"][0]["content"][0]["text"].as_str(),
            Some("internal steps")
        );
        assert_eq!(value["output"][1]["type"].as_str(), Some("message"));
        assert_eq!(
            value.get("output_text").and_then(Value::as_str),
            Some("hello")
        );
        assert!(value["usage"].is_null());
    }

    #[test]
    fn translates_chat_json_response_with_minimax_reasoning_details() {
        let body = br#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"MiniMax-M3",
            "choices":[{"message":{"reasoning_details":[{"text":"internal steps"}],"content":"hello"}}]
        }"#;
        let value =
            serde_json::from_slice::<Value>(&chat_response_to_responses(body).unwrap()).unwrap();

        assert_eq!(value["output"][0]["type"].as_str(), Some("reasoning"));
        assert_eq!(
            value["output"][0]["content"][0]["text"].as_str(),
            Some("internal steps")
        );
        assert_eq!(value["output"][1]["type"].as_str(), Some("message"));
    }

    #[test]
    fn translates_chat_tool_call_response() {
        let body = br#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"gpt-test",
            "choices":[{
                "message":{
                    "content":null,
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"get_weather","arguments":"{\"city\":\"Boston\"}"}
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        }"#;
        let value =
            serde_json::from_slice::<Value>(&chat_response_to_responses(body).unwrap()).unwrap();

        assert_eq!(value.get("output_text").and_then(Value::as_str), Some(""));
        assert_eq!(value["output"][0]["type"].as_str(), Some("function_call"));
        assert_eq!(value["output"][0]["call_id"].as_str(), Some("call_1"));
        assert_eq!(value["output"][0]["name"].as_str(), Some("get_weather"));
    }

    #[test]
    fn repairs_chat_tool_call_arguments_from_tool_call_markup() {
        let body = r#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"mimo-test",
            "choices":[{
                "message":{
                    "content":"<tool_call>\n<function=search_stocks>\n<parameter=query>正泰电源</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>",
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"search_stocks","arguments":"{\"query\": "}
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        }"#;
        let value =
            serde_json::from_slice::<Value>(&chat_response_to_responses(body.as_bytes()).unwrap())
                .unwrap();

        let function_call = value["output"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"].as_str() == Some("function_call"))
            .unwrap();
        assert_eq!(
            function_call["arguments"].as_str(),
            Some("{\"limit\":5,\"query\":\"正泰电源\"}")
        );
    }

    #[test]
    fn rejects_unrepairable_chat_tool_call_arguments() {
        let body = br#"{
            "id":"chatcmpl_123",
            "created":123,
            "model":"mimo-test",
            "choices":[{
                "message":{
                    "content":"plain text",
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"search_stocks","arguments":"{\"query\": "}
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        }"#;
        let err = chat_response_to_responses(body).unwrap_err();

        assert_eq!(err.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err.code, "invalid_upstream_response");
    }
}
