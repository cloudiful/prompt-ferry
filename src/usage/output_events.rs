use serde_json::Value;

pub(super) fn has_output_event(value: &Value) -> bool {
    let event_type = value.get("type").and_then(Value::as_str);
    if event_type.is_some_and(is_lifecycle_event) {
        return false;
    }

    if let Some(event_type) = event_type {
        if matches!(
            event_type,
            "response.output_text.delta"
                | "response.reasoning_text.delta"
                | "response.reasoning_summary_text.delta"
                | "response.function_call_arguments.delta"
                | "response.custom_tool_call_input.delta"
                | "response.refusal.delta"
        ) {
            return has_non_empty_output_value(value);
        }
        if matches!(
            event_type,
            "response.output_item.added" | "response.content_part.added"
        ) {
            return value
                .get("item")
                .or_else(|| value.get("part"))
                .is_some_and(has_non_empty_output_value);
        }
        if event_type == "content_block_delta" {
            return value.get("delta").is_some_and(has_non_empty_output_value);
        }
    }

    if value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| choices.iter().any(choice_has_output))
    {
        return true;
    }
    if value
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(output_item_has_output))
    {
        return true;
    }
    value.get("delta").is_some_and(has_non_empty_output_value)
}

fn is_lifecycle_event(event_type: &str) -> bool {
    matches!(
        event_type.rsplit('.').next(),
        Some("created" | "in_progress" | "completed" | "failed" | "incomplete")
    )
}

fn has_non_empty_output_value(value: &Value) -> bool {
    match value {
        Value::String(value) => !value.is_empty(),
        Value::Array(values) => values.iter().any(has_non_empty_output_value),
        Value::Object(object) => {
            object
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|tool_calls| !tool_calls.is_empty())
                || object
                    .get("function_call")
                    .is_some_and(has_non_empty_output_value)
                || [
                    "delta",
                    "text",
                    "output_text",
                    "reasoning",
                    "reasoning_content",
                    "thinking",
                    "arguments",
                    "partial_json",
                    "input",
                    "refusal",
                    "content",
                ]
                .iter()
                .filter_map(|key| object.get(*key))
                .any(has_non_empty_output_value)
        }
        _ => false,
    }
}

fn choice_has_output(choice: &Value) -> bool {
    let Some(delta) = choice.get("delta") else {
        return choice
            .get("message")
            .is_some_and(has_non_empty_output_value);
    };
    has_non_empty_output_value(delta)
}

fn output_item_has_output(item: &Value) -> bool {
    item.get("content")
        .or_else(|| item.get("arguments"))
        .or_else(|| item.get("input"))
        .or_else(|| item.get("refusal"))
        .is_some_and(has_non_empty_output_value)
        || item
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|item_type| {
                item_type.contains("function_call")
                    || item_type.contains("custom_tool_call")
                    || item_type.contains("computer_call")
            })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::has_output_event;

    #[test]
    fn recognizes_reasoning_tool_refusal_and_provider_output_events() {
        for event in [
            json!({ "type": "response.output_text.delta", "delta": "hello" }),
            json!({ "type": "response.reasoning_text.delta", "delta": "think" }),
            json!({ "type": "response.reasoning_summary_text.delta", "delta": "think" }),
            json!({ "type": "response.function_call_arguments.delta", "delta": "{\"q\":" }),
            json!({ "type": "response.custom_tool_call_input.delta", "delta": "{\"q\":" }),
            json!({ "type": "response.refusal.delta", "delta": "cannot" }),
            json!({
                "choices": [{ "delta": { "reasoning_content": "think" } }]
            }),
            json!({
                "choices": [{ "delta": { "tool_calls": [{ "index": 0 }] } }]
            }),
            json!({
                "type": "content_block_delta",
                "delta": { "type": "thinking_delta", "thinking": "think" }
            }),
            json!({
                "type": "content_block_delta",
                "delta": { "type": "input_json_delta", "partial_json": "{\"q\":" }
            }),
        ] {
            assert!(has_output_event(&event), "event={event}");
        }
    }

    #[test]
    fn ignores_response_lifecycle_and_usage_only_events() {
        assert!(!has_output_event(&json!({
            "type": "response.completed",
            "response": { "output": [{ "type": "message", "content": [{ "text": "done" }] }] }
        })));
        assert!(!has_output_event(&json!({ "type": "response.created" })));
        assert!(!has_output_event(
            &json!({ "usage": { "output_tokens": 20 } })
        ));
    }
}
