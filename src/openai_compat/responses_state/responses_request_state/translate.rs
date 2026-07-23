use super::*;

pub(super) fn to_chat_request_with_prefix(
    request: &NormalizedResponsesRequest,
    prefix_messages: &[Value],
) -> Result<Vec<u8>, CompatError> {
    let mut messages = Vec::new();
    if let Some(instructions) = request.chat_compat_instructions()? {
        messages.push(json!({
            "role": "system",
            "content": instructions,
        }));
    }
    messages.extend_from_slice(prefix_messages);
    messages.extend(translate_input(&Value::Array(request.chat_compat_items()))?);
    if messages.is_empty() {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "responses input must contain at least one supported message",
        ));
    }

    let mut chat = Map::new();
    for field in [
        "model",
        "prompt_cache_key",
        "temperature",
        "top_p",
        "stream",
    ] {
        if let Some(value) = request.object.get(field) {
            chat.insert(field.to_string(), value.clone());
        }
    }
    if let Some(value) = request.object.get("max_output_tokens") {
        chat.insert("max_tokens".to_string(), value.clone());
    }
    if let Some(value) = request
        .object
        .get("text")
        .filter(|value| has_meaningful_value(value))
    {
        chat.insert(
            "response_format".to_string(),
            super::request::translate_text_format(value)?,
        );
    }
    if let Some(value) = request
        .object
        .get("reasoning")
        .filter(|value| has_meaningful_value(value))
        && let Some(reasoning_effort) = super::request::translate_reasoning(value)?
    {
        chat.insert("reasoning_effort".to_string(), reasoning_effort);
    }
    if let Some(value) = request
        .object
        .get("tools")
        .filter(|value| has_meaningful_value(value))
    {
        chat.insert("tools".to_string(), translate_tools(value)?);
    }
    if let Some(value) = request
        .object
        .get("tool_choice")
        .filter(|value| has_meaningful_value(value))
    {
        chat.insert("tool_choice".to_string(), translate_tool_choice(value)?);
    }
    if let Some(value) = request
        .object
        .get("parallel_tool_calls")
        .filter(|value| has_meaningful_value(value))
    {
        chat.insert("parallel_tool_calls".to_string(), value.clone());
    }
    chat.insert("messages".to_string(), Value::Array(messages));

    serde_json::to_vec(&Value::Object(chat)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated chat request",
        )
    })
}

pub(super) fn to_responses_request_with_prefix(
    request: &NormalizedResponsesRequest,
    prefix_items: &[Value],
    drop_item_references: bool,
    drop_conversation: bool,
) -> Result<Vec<u8>, CompatError> {
    let mut body = request.object.clone();
    body.remove("previous_response_id");
    if drop_conversation {
        body.remove("conversation");
    }
    if let Some(instructions) = request.instructions.as_deref() {
        body.insert(
            "instructions".to_string(),
            Value::String(instructions.to_string()),
        );
    } else {
        body.remove("instructions");
    }
    let mut input = prefix_items.to_vec();
    input.extend(
        request
            .items
            .iter()
            .filter(|item| {
                !drop_item_references
                    || item
                        .as_object()
                        .and_then(|object| object.get("type").and_then(Value::as_str))
                        != Some("item_reference")
            })
            .cloned(),
    );
    body.insert(
        "input".to_string(),
        Value::Array(normalize_responses_input_for_upstream(&input)?),
    );
    body.insert("store".to_string(), Value::Bool(true));
    serde_json::to_vec(&Value::Object(body)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated responses request",
        )
    })
}

pub(super) fn chat_compat_instructions(
    instructions: Option<&str>,
    items: &[Value],
) -> Result<Option<String>, CompatError> {
    let mut parts = Vec::new();
    if let Some(instructions) = instructions {
        parts.push(instructions.to_string());
    }
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let role = object.get("role").and_then(Value::as_str);
        if !matches!(role, Some("system" | "developer")) {
            continue;
        }
        let content = object.get("content").unwrap_or(item);
        let text = translate_content(content)?;
        let text = extract_text(&text);
        if text.trim().is_empty() {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "instruction messages must contain text content for the compatibility subset",
            ));
        }
        parts.push(text);
    }
    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("\n\n")))
    }
}

pub(super) fn chat_compat_items(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter(|item| {
            let Some(object) = item.as_object() else {
                return true;
            };
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|item_type| item_type == "item_reference")
            {
                return false;
            }
            !object
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| matches!(role, "system" | "developer"))
        })
        .cloned()
        .collect()
}
