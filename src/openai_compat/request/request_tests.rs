#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::super::{
        chat_request_to_responses, conversation_key, previous_response_id,
        request_translate::responses_request_to_chat_with_prefix, responses_request_to_chat,
    };

    #[test]
    fn translates_chat_request_with_mixed_images_and_detail_to_responses() {
        let value = serde_json::from_slice::<Value>(
            &chat_request_to_responses(
                br#"{
                    "model":"vision-test",
                    "messages":[{
                        "role":"user",
                        "content":[
                            {"type":"text","text":"describe"},
                            {"type":"image_url","image_url":{"url":"https://example.com/a.png","detail":"high"}},
                            {"type":"image_url","image_url":"data:image/png;base64,AA=="}
                        ]
                    }]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        let content = value["input"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"].as_str(), Some("input_text"));
        assert_eq!(content[1]["type"].as_str(), Some("input_image"));
        assert_eq!(
            content[1]["image_url"].as_str(),
            Some("https://example.com/a.png")
        );
        assert_eq!(content[1]["detail"].as_str(), Some("high"));
        assert_eq!(
            content[2]["image_url"].as_str(),
            Some("data:image/png;base64,AA==")
        );
    }

    #[test]
    fn translates_basic_responses_request_to_chat() {
        let request = br#"{
            "model":"gpt-test",
            "instructions":"be terse",
            "input":[{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
            "tools":[{"name":"get_weather","description":"weather","parameters":{"type":"object"},"strict":true}],
            "tool_choice":{"type":"function","name":"get_weather"},
            "parallel_tool_calls":true,
            "stream":true,
            "max_output_tokens":4
        }"#;
        let value =
            serde_json::from_slice::<Value>(&responses_request_to_chat(request).unwrap()).unwrap();

        assert_eq!(value.get("model").and_then(Value::as_str), Some("gpt-test"));
        assert_eq!(value.get("max_tokens").and_then(Value::as_i64), Some(4));
        assert_eq!(value.get("stream").and_then(Value::as_bool), Some(true));
        assert_eq!(
            value.get("parallel_tool_calls").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(value["tools"][0]["type"].as_str(), Some("function"));
        assert_eq!(
            value["tools"][0]["function"]["name"].as_str(),
            Some("get_weather")
        );
        assert_eq!(
            value["tool_choice"]["function"]["name"].as_str(),
            Some("get_weather")
        );
        assert_eq!(value["messages"][0]["role"].as_str(), Some("system"));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("user"));
    }

    #[test]
    fn translates_mixed_text_and_image_parts_to_chat_image_url_parts() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"vision-test",
                    "input":[{
                        "role":"user",
                        "content":[
                            {"type":"input_text","text":"describe these images"},
                            {"type":"input_image","image_url":"https://example.com/chart.png"},
                            {"type":"input_image","image_url":{"url":"data:image/png;base64,AA==","detail":"high"}}
                        ]
                    }]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        let content = value["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["type"].as_str(), Some("text"));
        assert_eq!(content[0]["text"].as_str(), Some("describe these images"));
        assert_eq!(content[1]["type"].as_str(), Some("image_url"));
        assert_eq!(
            content[1]["image_url"]["url"].as_str(),
            Some("https://example.com/chart.png")
        );
        assert_eq!(content[2]["type"].as_str(), Some("image_url"));
        assert_eq!(
            content[2]["image_url"]["url"].as_str(),
            Some("data:image/png;base64,AA==")
        );
        assert_eq!(content[2]["image_url"]["detail"].as_str(), Some("high"));
    }

    #[test]
    fn translates_function_call_history_to_chat_messages() {
        let request = br#"{
            "model":"gpt-test",
            "input":[
                {"role":"user","content":"check weather"},
                {"type":"function_call","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Boston\"}"},
                {"type":"function_call_output","call_id":"call_1","output":[{"type":"input_text","text":"72F"}]}
            ]
        }"#;
        let value =
            serde_json::from_slice::<Value>(&responses_request_to_chat(request).unwrap()).unwrap();

        assert_eq!(value["messages"][1]["role"].as_str(), Some("assistant"));
        assert_eq!(
            value["messages"][1]["tool_calls"][0]["id"].as_str(),
            Some("call_1")
        );
        assert_eq!(
            value["messages"][1]["tool_calls"][0]["function"]["name"].as_str(),
            Some("get_weather")
        );
        assert_eq!(value["messages"][2]["role"].as_str(), Some("tool"));
        assert_eq!(
            value["messages"][2]["tool_call_id"].as_str(),
            Some("call_1")
        );
        assert_eq!(
            value["messages"][2]["content"][0]["text"].as_str(),
            Some("72F")
        );
    }

    #[test]
    fn groups_parallel_function_calls_into_one_assistant_message() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"gpt-test",
                    "input":[
                        {"role":"user","content":"check several values"},
                        {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"key\":1}"},
                        {"type":"function_call","call_id":"call_2","name":"lookup","arguments":"{\"key\":2}"},
                        {"type":"function_call","call_id":"call_3","name":"lookup","arguments":"{\"key\":3}"},
                        {"type":"function_call","call_id":"call_4","name":"lookup","arguments":"{\"key\":4}"},
                        {"type":"function_call_output","call_id":"call_1","output":"one"},
                        {"type":"function_call_output","call_id":"call_2","output":"two"},
                        {"type":"function_call_output","call_id":"call_3","output":"three"},
                        {"type":"function_call_output","call_id":"call_4","output":"four"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"].as_array().map(Vec::len), Some(6));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("assistant"));
        assert_eq!(
            value["messages"][1]["tool_calls"].as_array().map(Vec::len),
            Some(4)
        );
        for (index, call_id) in ["call_1", "call_2", "call_3", "call_4"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                value["messages"][1]["tool_calls"][index]["id"].as_str(),
                Some(call_id)
            );
            assert_eq!(value["messages"][index + 2]["role"].as_str(), Some("tool"));
            assert_eq!(
                value["messages"][index + 2]["tool_call_id"].as_str(),
                Some(call_id)
            );
        }
    }

    #[test]
    fn splits_tool_call_rounds_after_outputs() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "input":[
                        {"role":"user","content":"check values"},
                        {"type":"function_call","call_id":"call_1","name":"first","arguments":"{}"},
                        {"type":"function_call_output","call_id":"call_1","output":"one"},
                        {"type":"function_call","call_id":"call_2","name":"second","arguments":"{}"},
                        {"type":"function_call_output","call_id":"call_2","output":"two"},
                        {"role":"user","content":"continue"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"].as_array().map(Vec::len), Some(6));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("assistant"));
        assert_eq!(
            value["messages"][1]["tool_calls"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            value["messages"][1]["tool_calls"][0]["id"].as_str(),
            Some("call_1")
        );
        assert_eq!(
            value["messages"][2]["tool_call_id"].as_str(),
            Some("call_1")
        );
        assert_eq!(value["messages"][3]["role"].as_str(), Some("assistant"));
        assert_eq!(
            value["messages"][3]["tool_calls"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            value["messages"][3]["tool_calls"][0]["id"].as_str(),
            Some("call_2")
        );
        assert_eq!(
            value["messages"][4]["tool_call_id"].as_str(),
            Some("call_2")
        );
        assert_eq!(value["messages"][5]["role"].as_str(), Some("user"));
        assert_eq!(value["messages"][5]["content"].as_str(), Some("continue"));
    }

    #[test]
    fn does_not_merge_tool_calls_across_role_messages() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "input":[
                        {"type":"function_call","call_id":"call_1","name":"first","arguments":"{}"},
                        {"role":"user","content":"between calls"},
                        {"type":"function_call","call_id":"call_2","name":"second","arguments":"{}"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            value["messages"][0]["tool_calls"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(value["messages"][1]["role"].as_str(), Some("user"));
        assert_eq!(
            value["messages"][2]["tool_calls"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            value["messages"][2]["tool_calls"][0]["id"].as_str(),
            Some("call_2")
        );
    }

    #[test]
    fn unwraps_message_wrappers_inside_user_content() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"gpt-test",
                    "input":[{
                        "role":"user",
                        "content":{
                            "type":"message",
                            "role":"user",
                            "content":[{"type":"input_text","text":"wrapped"}]
                        }
                    }]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(
            value["messages"][0]["content"][0]["text"].as_str(),
            Some("wrapped")
        );
    }

    #[test]
    fn unwraps_message_wrappers_inside_assistant_content() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"deepseek-reasoning",
                    "input":[
                        {
                            "role":"assistant",
                            "content":{
                                "type":"message",
                                "role":"assistant",
                                "content":[
                                    {
                                        "type":"reasoning",
                                        "content":[{"type":"reasoning_text","text":"plan first"}]
                                    },
                                    {"type":"output_text","text":"done"}
                                ]
                            }
                        },
                        {"role":"user","content":"continue"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            value["messages"][0]["content"][0]["text"].as_str(),
            Some("done")
        );
        assert_eq!(
            value["messages"][0]["reasoning_content"].as_str(),
            Some("plan first")
        );
    }

    #[test]
    fn translates_assistant_reasoning_parts_to_reasoning_content() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"deepseek-reasoning",
                    "input":[
                        {
                            "role":"assistant",
                            "content":[
                                {
                                    "type":"reasoning",
                                    "content":[
                                        {"type":"reasoning_text","text":"plan "},
                                        {"type":"reasoning_text","text":"then answer"}
                                    ]
                                },
                                {"type":"output_text","text":"working"}
                            ]
                        },
                        {"role":"user","content":"continue"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"][0]["role"].as_str(), Some("assistant"));
        assert_eq!(
            value["messages"][0]["reasoning_content"].as_str(),
            Some("plan then answer")
        );
        assert_eq!(
            value["messages"][0]["content"][0]["text"].as_str(),
            Some("working")
        );
    }

    #[test]
    fn keeps_reasoning_only_assistant_messages_with_null_content() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "model":"deepseek-reasoning",
                    "input":[
                        {
                            "role":"assistant",
                            "content":[
                                {
                                    "type":"reasoning",
                                    "content":[{"type":"reasoning_text","text":"hidden chain"}]
                                }
                            ]
                        },
                        {"role":"user","content":"continue"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert!(value["messages"][0]["content"].is_null());
        assert_eq!(
            value["messages"][0]["reasoning_content"].as_str(),
            Some("hidden chain")
        );
    }

    #[test]
    fn allows_previous_response_id_for_replay_aware_callers() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(br#"{"previous_response_id":"resp_123","input":"hi"}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"][0]["content"].as_str(), Some("hi"));
    }

    #[test]
    fn allows_conversation_for_replay_aware_callers() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(br#"{"conversation":"conv_123","input":"hi"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"][0]["content"].as_str(), Some("hi"));
    }

    #[test]
    fn accepts_include_for_chat_native_compat_and_ignores_it() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "input":"hi",
                    "include":[
                        "file_search_call.results",
                        "reasoning.encrypted_content"
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"][0]["content"].as_str(), Some("hi"));
        assert!(value.get("include").is_none());
    }

    #[test]
    fn forwards_prompt_cache_key_to_chat_native_compat() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "input":"hi",
                    "prompt_cache_key":"thread-123"
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"][0]["content"].as_str(), Some("hi"));
        assert_eq!(value["prompt_cache_key"].as_str(), Some("thread-123"));
    }

    #[test]
    fn translates_reasoning_effort_to_chat_native_field() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{
                    "input":"hi",
                    "reasoning":{"effort":"low"}
                }"#,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(value["messages"][0]["content"].as_str(), Some("hi"));
        assert_eq!(value["reasoning_effort"].as_str(), Some("low"));
    }

    #[test]
    fn rejects_conversation_with_previous_response_id() {
        let err = responses_request_to_chat(
            br#"{"conversation":"conv_123","previous_response_id":"resp_123","input":"hi"}"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_request");
        assert!(
            err.message
                .contains("conversation and previous_response_id")
        );
    }

    #[test]
    fn prepends_prefix_messages() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat_with_prefix(
                br#"{"input":"next"}"#,
                &[serde_json::json!({"role":"assistant","content":"prev"})],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"][0]["role"].as_str(), Some("assistant"));
        assert_eq!(value["messages"][1]["content"].as_str(), Some("next"));
    }

    #[test]
    fn places_instructions_before_prefix_messages_on_replay() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat_with_prefix(
                br#"{"instructions":"be terse","input":[{"type":"function_call_output","call_id":"call_1","output":"72F"}]}"#,
                &[serde_json::json!({
                    "role":"assistant",
                    "content": Value::Null,
                    "tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Boston\"}"}}]
                })],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"][0]["role"].as_str(), Some("system"));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("assistant"));
        assert_eq!(value["messages"][2]["role"].as_str(), Some("tool"));
        assert_eq!(
            value["messages"][2]["tool_call_id"].as_str(),
            Some("call_1")
        );
    }

    #[test]
    fn drops_item_references_from_chat_native_replay_inputs() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat_with_prefix(
                br#"{"previous_response_id":"resp_123","input":[{"type":"item_reference","id":"call_1"},{"type":"function_call_output","call_id":"call_1","output":"72F"}]}"#,
                &[serde_json::json!({
                    "role":"assistant",
                    "content": Value::Null,
                    "tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Boston\"}"}}]
                })],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["messages"][0]["role"].as_str(), Some("assistant"));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("tool"));
        assert_eq!(
            value["messages"][1]["tool_call_id"].as_str(),
            Some("call_1")
        );
    }

    #[test]
    fn drops_role_messages_whose_content_only_contains_item_references() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{"previous_response_id":"resp_123","input":[{"role":"user","content":[{"type":"item_reference","id":"msg_1"}]},{"role":"user","content":"next"}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(value["messages"][0]["content"].as_str(), Some("next"));
    }

    #[test]
    fn strips_item_references_from_role_message_content_arrays() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{"previous_response_id":"resp_123","input":[{"role":"user","content":[{"type":"item_reference","id":"msg_1"},{"type":"input_text","text":"next"}]}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(
            value["messages"][0]["content"][0]["type"].as_str(),
            Some("text")
        );
        assert_eq!(
            value["messages"][0]["content"][0]["text"].as_str(),
            Some("next")
        );
    }

    #[test]
    fn reads_previous_response_id() {
        assert_eq!(
            previous_response_id(br#"{"previous_response_id":"resp_123","input":"hi"}"#).as_deref(),
            Some("resp_123")
        );
    }

    #[test]
    fn reads_conversation_key() {
        assert_eq!(
            conversation_key(br#"{"conversation":"conv_123","input":"hi"}"#).as_deref(),
            Some("conv_123")
        );
    }

    #[test]
    fn rejects_unknown_meaningful_root_fields() {
        let err =
            responses_request_to_chat(br#"{"input":"hi","metadata":{"trace":"abc"}}"#).unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("metadata"));
    }

    #[test]
    fn translates_text_format_text_to_response_format() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(br#"{"input":"hi","text":{"format":{"type":"text"}}}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["response_format"]["type"].as_str(), Some("text"));
    }

    #[test]
    fn translates_text_format_json_schema_to_response_format() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{"input":"hi","text":{"format":{"type":"json_schema","name":"answer_schema","description":"shape","schema":{"type":"object"},"strict":true}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["response_format"]["type"].as_str(),
            Some("json_schema")
        );
        assert_eq!(
            value["response_format"]["json_schema"]["name"].as_str(),
            Some("answer_schema")
        );
        assert_eq!(
            value["response_format"]["json_schema"]["strict"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn translates_text_format_json_object_to_response_format() {
        let value = serde_json::from_slice::<Value>(
            &responses_request_to_chat(
                br#"{"input":"hi","text":{"format":{"type":"json_object"}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["response_format"]["type"].as_str(),
            Some("json_object")
        );
    }

    #[test]
    fn rejects_invalid_text_config_shapes() {
        let err = responses_request_to_chat(br#"{"input":"hi","text":"json"}"#).unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text config"));

        let err = responses_request_to_chat(br#"{"input":"hi","text":{}}"#).unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text.format is required"));

        let err =
            responses_request_to_chat(br#"{"input":"hi","text":{"format":"json"}}"#).unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text.format must be an object"));
    }

    #[test]
    fn rejects_unsupported_text_format_variants() {
        let err = responses_request_to_chat(br#"{"input":"hi","text":{"format":{"type":"xml"}}}"#)
            .unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("type `xml`"));

        let err = responses_request_to_chat(
            br#"{"input":"hi","text":{"verbosity":"high","format":{"type":"text"}}}"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text.verbosity"));
    }

    #[test]
    fn rejects_invalid_json_schema_text_format() {
        let err = responses_request_to_chat(
            br#"{"input":"hi","text":{"format":{"type":"json_schema","schema":{"type":"object"}}}}"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text.format.name"));

        let err = responses_request_to_chat(
            br#"{"input":"hi","text":{"format":{"type":"json_schema","name":"answer","schema":{"type":"object"},"extra":true}}}"#,
        )
        .unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("text.format.extra"));
    }

    #[test]
    fn rejects_unsupported_reasoning_fields() {
        let err = responses_request_to_chat(br#"{"input":"hi","reasoning":{"summary":"auto"}}"#)
            .unwrap_err();
        assert_eq!(err.code, "unsupported_feature");
        assert!(err.message.contains("reasoning.summary"));
    }
}
