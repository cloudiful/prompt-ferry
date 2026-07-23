use http::StatusCode;
use serde_json::Value;

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
