use super::*;

pub(crate) fn output_items_to_assistant_message(
    output_items: &[Value],
    fallback: Option<&Value>,
) -> Result<Value, CompatError> {
    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for item in output_items {
        let object = item.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "responses output items must be JSON objects",
            )
        })?;
        match object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message")
        {
            "message" => {
                let text = object.get("content").map(extract_text).unwrap_or_default();
                if !text.is_empty() {
                    content_parts.push(text);
                }
            }
            "function_call" => {
                tool_calls.push(json!({
                    "id": object.get("call_id").and_then(Value::as_str).unwrap_or_default(),
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

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    let content = if content_parts.is_empty() {
        Value::Null
    } else {
        Value::String(content_parts.join("\n"))
    };
    message.insert("content".to_string(), content);
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if let Some(fallback) = fallback.and_then(Value::as_object)
        && let Some(reasoning) = fallback.get("reasoning_content").cloned()
        && has_meaningful_value(&reasoning)
    {
        message.insert("reasoning_content".to_string(), reasoning);
    }
    if let Some(fallback) = fallback.and_then(Value::as_object)
        && let Some(reasoning_details) = fallback.get("reasoning_details").cloned()
        && has_meaningful_value(&reasoning_details)
    {
        message.insert("reasoning_details".to_string(), reasoning_details);
    }
    Ok(Value::Object(message))
}

pub(crate) fn assistant_message_to_output_items(
    message: &Value,
) -> Result<Vec<Value>, CompatError> {
    chat_output_items_from_message(message)
}

pub(crate) fn extract_output_items_from_responses_value(
    value: &Value,
) -> Result<Vec<Value>, CompatError> {
    Ok(value
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub(crate) fn persisted_artifact(
    assistant_message: Option<Value>,
    output_items: Vec<Value>,
) -> Option<(Value, bool, bool)> {
    let assistant_message =
        assistant_message.map(|message| sync_tool_calls_with_output_items(message, &output_items));
    let has_reasoning_content = assistant_message
        .as_ref()
        .and_then(|message| message.get("reasoning_content"))
        .is_some_and(has_meaningful_value);
    let has_tool_calls = assistant_message
        .as_ref()
        .and_then(|message| message.get("tool_calls"))
        .is_some_and(has_meaningful_value)
        || output_items
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call"));
    if assistant_message.is_none() && output_items.is_empty() {
        return None;
    }
    Some((
        json!({
            "version": 1,
            "assistant_message": assistant_message,
            "output_items": output_items,
        }),
        has_reasoning_content,
        has_tool_calls,
    ))
}

fn sync_tool_calls_with_output_items(
    mut assistant_message: Value,
    output_items: &[Value],
) -> Value {
    let Some(object) = assistant_message.as_object_mut() else {
        return assistant_message;
    };
    let tool_calls = output_items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(|item| {
            json!({
                "id": item.get("call_id").and_then(Value::as_str).unwrap_or_default(),
                "type": "function",
                "function": {
                    "name": item.get("name").and_then(Value::as_str).unwrap_or_default(),
                    "arguments": item.get("arguments").and_then(Value::as_str).unwrap_or_default(),
                }
            })
        })
        .collect::<Vec<_>>();
    if tool_calls.is_empty() {
        return assistant_message;
    }
    object.insert("tool_calls".to_string(), Value::Array(tool_calls));
    assistant_message
}

pub(crate) fn persisted_output_items(message_json: &Value) -> Result<Vec<Value>, CompatError> {
    if let Some(version) = message_json.get("version").and_then(Value::as_i64)
        && version == 1
    {
        return Ok(message_json
            .get("output_items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default());
    }
    assistant_message_to_output_items(message_json)
}

pub(crate) fn persisted_assistant_message(message_json: &Value) -> Result<Value, CompatError> {
    if let Some(version) = message_json.get("version").and_then(Value::as_i64)
        && version == 1
    {
        if let Some(message) = message_json.get("assistant_message")
            && message.is_object()
        {
            return Ok(message.clone());
        }
        let output_items = persisted_output_items(message_json)?;
        return output_items_to_assistant_message(&output_items, None);
    }
    let mut message = message_json.as_object().cloned().unwrap_or_default();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if !message.contains_key("content") {
        message.insert("content".to_string(), Value::Null);
    }
    Ok(Value::Object(message))
}

pub(crate) fn responses_stream_output_items(chunks: &[Value]) -> Result<Vec<Value>, CompatError> {
    let mut output_by_index = HashMap::<usize, Value>::new();
    let mut completed_output = None;
    for value in chunks {
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            continue;
        };
        match event_type {
            "response.output_item.added" => {
                if let (Some(index), Some(item)) = (
                    value.get("output_index").and_then(Value::as_u64),
                    value.get("item"),
                ) {
                    output_by_index.insert(index as usize, item.clone());
                }
            }
            "response.output_text.delta" => {
                if let Some(index) = value.get("output_index").and_then(Value::as_u64)
                    && let Some(delta) = value.get("delta").and_then(Value::as_str)
                {
                    append_output_text(
                        output_by_index.entry(index as usize).or_insert_with(|| {
                            json!({
                                "type":"message",
                                "role":"assistant",
                                "content":[{
                                    "type":"output_text",
                                    "text":"",
                                    "annotations":[],
                                    "logprobs":[]
                                }]
                            })
                        }),
                        delta,
                    );
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(index) = value.get("output_index").and_then(Value::as_u64)
                    && let Some(delta) = value.get("delta").and_then(Value::as_str)
                {
                    let item = output_by_index.entry(index as usize).or_insert_with(|| {
                        json!({
                            "type":"function_call",
                            "call_id": value.get("call_id").and_then(Value::as_str).unwrap_or_default(),
                            "name":"",
                            "arguments":""
                        })
                    });
                    let current = item
                        .as_object()
                        .and_then(|object| object.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let next = format!("{current}{delta}");
                    if let Some(object) = item.as_object_mut() {
                        object.insert("arguments".to_string(), Value::String(next));
                    }
                }
            }
            "response.completed" => {
                completed_output = value
                    .get("response")
                    .and_then(|response| response.get("output"))
                    .and_then(Value::as_array)
                    .cloned();
            }
            _ => {}
        }
    }
    if let Some(output) = completed_output {
        return Ok(output);
    }
    let mut ordered = output_by_index.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(index, _)| *index);
    Ok(ordered.into_iter().map(|(_, item)| item).collect())
}

fn append_output_text(item: &mut Value, delta: &str) {
    let Some(content) = item
        .as_object_mut()
        .and_then(|object| object.get_mut("content"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let Some(first) = content.first_mut() else {
        return;
    };
    let next = format!(
        "{}{}",
        first
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        delta
    );
    if let Some(object) = first.as_object_mut() {
        object.insert("text".to_string(), Value::String(next));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{normalize_response_error, output_items_to_input_items};

    #[test]
    fn wraps_detail_errors_in_openai_shape() {
        let value =
            normalize_response_error(r#"{"detail":"Unsupported parameter: previous_response_id"}"#);
        assert_eq!(
            value["error"]["message"].as_str(),
            Some("Unsupported parameter: previous_response_id")
        );
    }

    #[test]
    fn preserves_existing_openai_error_shape() {
        let value = normalize_response_error(
            r#"{"error":{"message":"bad","type":"invalid_request_error","param":null,"code":null}}"#,
        );
        assert_eq!(value["error"]["message"].as_str(), Some("bad"));
    }

    #[test]
    fn preserves_top_level_error_metadata_when_wrapping_message() {
        let value = normalize_response_error(
            r#"{"message":"error code: 502","type":"invalid_request_error","param":null,"code":"bad_gateway"}"#,
        );
        assert_eq!(value["error"]["message"].as_str(), Some("error code: 502"));
        assert_eq!(
            value["error"]["type"].as_str(),
            Some("invalid_request_error")
        );
        assert_eq!(value["error"]["code"].as_str(), Some("bad_gateway"));
        assert!(value["error"]["param"].is_null());
    }

    #[test]
    fn preserves_top_level_error_metadata_when_wrapping_detail() {
        let value = normalize_response_error(
            r#"{"detail":"upstream overloaded","type":"server_error","param":"model","code":"bad_gateway"}"#,
        );
        assert_eq!(
            value["error"]["message"].as_str(),
            Some("upstream overloaded")
        );
        assert_eq!(value["error"]["type"].as_str(), Some("server_error"));
        assert_eq!(value["error"]["code"].as_str(), Some("bad_gateway"));
        assert_eq!(value["error"]["param"].as_str(), Some("model"));
    }

    #[test]
    fn builds_assistant_message_from_output_items() {
        let items = vec![
            json!({"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}),
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}),
        ];
        let message = super::output_items_to_assistant_message(&items, None).unwrap();
        assert_eq!(message["content"].as_str(), Some("done"));
        assert_eq!(message["tool_calls"][0]["id"].as_str(), Some("call_1"));
    }

    #[test]
    fn replays_assistant_output_items_as_output_text_parts() {
        let items = vec![json!({
            "type":"message",
            "role":"assistant",
            "content":[{"type":"output_text","text":"done"}]
        })];
        let input_items = output_items_to_input_items(&items).unwrap();
        assert_eq!(input_items[0]["role"].as_str(), Some("assistant"));
        assert_eq!(
            input_items[0]["content"][0]["type"].as_str(),
            Some("output_text")
        );
        assert_eq!(input_items[0]["content"][0]["text"].as_str(), Some("done"));
    }

    #[test]
    fn replays_function_calls_as_explicit_calls() {
        let items = vec![json!({
            "id":"fc_123",
            "type":"function_call",
            "call_id":"call_1",
            "name":"lookup",
            "arguments":"{}"
        })];
        let input_items = output_items_to_input_items(&items).unwrap();
        assert_eq!(input_items[0]["type"].as_str(), Some("function_call"));
        assert_eq!(input_items[0]["call_id"].as_str(), Some("call_1"));
        assert_eq!(input_items[0]["name"].as_str(), Some("lookup"));
        assert_eq!(input_items[0]["arguments"].as_str(), Some("{}"));
    }

    #[test]
    fn persisted_artifact_aligns_assistant_tool_calls_with_repaired_output_items() {
        let assistant_message = json!({
            "role":"assistant",
            "content":"<tool_call>\n<function=search_stocks>\n<parameter=query>正泰电源</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>",
            "tool_calls":[{
                "id":"call_1",
                "type":"function",
                "function":{"name":"search_stocks","arguments":"{\"query\": "}
            }]
        });
        let output_items = vec![json!({
            "id":"call_1",
            "type":"function_call",
            "call_id":"call_1",
            "name":"search_stocks",
            "arguments":"{\"limit\":5,\"query\":\"正泰电源\"}"
        })];

        let (artifact, _, _) =
            super::persisted_artifact(Some(assistant_message), output_items).unwrap();
        assert_eq!(
            artifact["assistant_message"]["tool_calls"][0]["function"]["arguments"].as_str(),
            Some("{\"limit\":5,\"query\":\"正泰电源\"}")
        );
    }
}
