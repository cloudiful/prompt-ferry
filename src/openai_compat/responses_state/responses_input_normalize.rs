use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ItemKind<'a> {
    RoleMessage(&'a str),
    FunctionCall,
    ItemReference,
    FunctionCallOutput,
    PartOnly,
}

pub(super) fn item_kind<'a>(object: &'a Map<String, Value>) -> Result<ItemKind<'a>, CompatError> {
    if let Some(item_type) = object.get("type").and_then(Value::as_str) {
        return Ok(match item_type {
            "function_call" => ItemKind::FunctionCall,
            "item_reference" => ItemKind::ItemReference,
            "function_call_output" => ItemKind::FunctionCallOutput,
            _ => {
                if object.get("role").is_some() {
                    ItemKind::RoleMessage(
                        object.get("role").and_then(Value::as_str).unwrap_or("user"),
                    )
                } else {
                    ItemKind::PartOnly
                }
            }
        });
    }
    Ok(
        if let Some(role) = object.get("role").and_then(Value::as_str) {
            ItemKind::RoleMessage(role)
        } else {
            ItemKind::PartOnly
        },
    )
}

pub(super) fn input_items_from_object(
    object: &Map<String, Value>,
) -> Result<Vec<Value>, CompatError> {
    let Some(input) = object.get("input") else {
        return Ok(Vec::new());
    };
    match input {
        Value::Array(items) => Ok(items.clone()),
        Value::String(text) => Ok(vec![json!({
            "role": "user",
            "content": text,
        })]),
        Value::Object(item) => {
            if item.get("role").is_some() || item.get("type").is_some() {
                Ok(vec![Value::Object(item.clone())])
            } else {
                Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "responses input item must be a role message, function call item, function result item, or supported text/image part",
                ))
            }
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses input item must be a string, object, or array",
        )),
    }
}

pub(super) fn normalize_instruction_messages(
    instructions: Option<String>,
    items: &mut Vec<Value>,
) -> Result<Option<String>, CompatError> {
    let mut lifted = Vec::new();
    while let Some(item) = items.first() {
        let Some(object) = item.as_object() else {
            break;
        };
        let role = object.get("role").and_then(Value::as_str);
        if !matches!(role, Some("system" | "developer")) {
            break;
        }
        if object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|item_type| item_type != "message")
        {
            break;
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
        lifted.push(text);
        items.remove(0);
    }

    let instructions = match (instructions, lifted.is_empty()) {
        (Some(existing), true) => Some(existing),
        (None, true) => None,
        (Some(existing), false) => {
            let mut parts = vec![existing];
            parts.extend(lifted);
            Some(parts.join("\n\n"))
        }
        (None, false) => Some(lifted.join("\n\n")),
    };
    Ok(instructions)
}

pub(super) fn invalid_continuation(message: impl Into<String>) -> CompatError {
    CompatError::new(
        StatusCode::BAD_REQUEST,
        "invalid_responses_continuation",
        message,
    )
}

pub(crate) fn output_items_to_input_items(
    output_items: &[Value],
) -> Result<Vec<Value>, CompatError> {
    let mut items = Vec::new();
    for item in output_items {
        let object = item.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "responses output items must be JSON objects",
            )
        })?;
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message");
        match item_type {
            "message" => {
                let role = object
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("assistant");
                let text = object.get("content").map(extract_text).unwrap_or_default();
                if !text.is_empty() {
                    let content = if role == "assistant" {
                        Value::Array(vec![assistant_output_text_part(&text)])
                    } else {
                        Value::String(text)
                    };
                    items.push(json!({ "role": role, "content": content }));
                }
            }
            "function_call" => {
                let call_id = object
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = object
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                items.push(json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                }));
            }
            "reasoning" => items.push(item.clone()),
            other => {
                return Err(CompatError::new(
                    StatusCode::BAD_GATEWAY,
                    "invalid_upstream_response",
                    format!("responses output item type `{other}` is not supported for replay"),
                ));
            }
        }
    }
    Ok(items)
}

pub(super) fn normalize_responses_input_for_upstream(
    items: &[Value],
) -> Result<Vec<Value>, CompatError> {
    items
        .iter()
        .map(normalize_responses_input_item_for_upstream)
        .collect()
}

fn normalize_responses_input_item_for_upstream(item: &Value) -> Result<Value, CompatError> {
    let Some(object) = item.as_object() else {
        return Ok(item.clone());
    };
    if object.get("role").and_then(Value::as_str) != Some("assistant") {
        return Ok(item.clone());
    }
    let mut normalized = object.clone();
    if let Some(content) = object.get("content") {
        normalized.insert(
            "content".to_string(),
            normalize_assistant_content_for_upstream(content)?,
        );
    }
    Ok(Value::Object(normalized))
}

fn normalize_assistant_content_for_upstream(content: &Value) -> Result<Value, CompatError> {
    match content {
        Value::Null => Ok(Value::Null),
        Value::String(text) => Ok(Value::Array(vec![assistant_output_text_part(text)])),
        Value::Array(parts) => Ok(Value::Array(
            parts
                .iter()
                .map(normalize_assistant_content_part_for_upstream)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(object) => {
            if object.get("type").is_some() {
                return Ok(Value::Array(vec![
                    normalize_assistant_content_part_for_upstream(content)?,
                ]));
            }
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return Ok(Value::Array(vec![assistant_output_text_part(text)]));
            }
            Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "assistant message content must be text or supported content parts",
            ))
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "assistant message content must be text or supported content parts",
        )),
    }
}

fn normalize_assistant_content_part_for_upstream(part: &Value) -> Result<Value, CompatError> {
    let object = part.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "assistant content parts must be JSON objects",
        )
    })?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("output_text");
    match part_type {
        "output_text" | "input_text" | "text" => {
            let text = object
                .get("text")
                .or_else(|| object.get("input_text"))
                .or_else(|| object.get("output_text"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "assistant text parts require a text field",
                    )
                })?;
            Ok(assistant_output_text_part(text))
        }
        "refusal" => Ok(part.clone()),
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("assistant content part type `{other}` is not supported"),
        )),
    }
}

pub(super) fn assistant_output_text_part(text: &str) -> Value {
    json!({
        "type": "output_text",
        "text": text,
    })
}

pub(super) fn required_string_field<'a>(
    object: &'a Map<String, Value>,
    keys: &[&str],
    message: &'static str,
) -> Result<&'a str, CompatError> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CompatError::new(StatusCode::BAD_REQUEST, "unsupported_feature", message))
}
