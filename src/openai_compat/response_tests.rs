#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::openai_compat::responses_response_to_chat;

    fn translate(value: Value) -> Value {
        let body = serde_json::to_vec(&value).unwrap();
        serde_json::from_slice(&responses_response_to_chat(&body).unwrap()).unwrap()
    }

    #[test]
    fn translates_text_reasoning_and_usage() {
        let response = translate(json!({
            "id": "resp_1",
            "created_at": 123,
            "model": "reasoning-test",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "think first"}]
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "hello"}]
                }
            ],
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5,
                "input_tokens_details": {"cached_tokens": 1},
                "output_tokens_details": {"reasoning_tokens": 2}
            }
        }));

        assert_eq!(response["object"], "chat.completion");
        assert_eq!(response["created"], 123);
        assert_eq!(response["model"], "reasoning-test");
        assert_eq!(response["choices"][0]["message"]["content"], "hello");
        assert_eq!(
            response["choices"][0]["message"]["reasoning_content"],
            "think first"
        );
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
        assert_eq!(response["usage"]["prompt_tokens"], 2);
        assert_eq!(response["usage"]["completion_tokens"], 3);
        assert_eq!(response["usage"]["total_tokens"], 5);
        assert_eq!(
            response["usage"]["prompt_tokens_details"]["cached_tokens"],
            1
        );
        assert_eq!(
            response["usage"]["completion_tokens_details"]["reasoning_tokens"],
            2
        );
    }

    #[test]
    fn translates_function_calls_to_chat_tool_calls() {
        let response = translate(json!({
            "id": "resp_tools",
            "model": "tool-test",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{\"q\":\"rust\"}"
            }]
        }));

        assert!(response["choices"][0]["message"]["content"].is_null());
        assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][0]["id"],
            "call_1"
        );
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"q\":\"rust\"}"
        );
    }

    #[test]
    fn maps_incomplete_responses_to_length() {
        let response = translate(json!({
            "id": "resp_incomplete",
            "model": "limit-test",
            "status": "incomplete",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "partial"}]
            }]
        }));

        assert_eq!(response["choices"][0]["message"]["content"], "partial");
        assert_eq!(response["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn falls_back_to_output_text_when_output_items_have_no_text() {
        let response = translate(json!({
            "id": "resp_fallback",
            "model": "fallback-test",
            "status": "completed",
            "output": [],
            "output_text": "fallback text"
        }));

        assert_eq!(
            response["choices"][0]["message"]["content"],
            "fallback text"
        );
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
    }
}
