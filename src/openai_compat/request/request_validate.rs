use super::*;

pub(crate) fn reject_present(
    object: &Map<String, Value>,
    key: &str,
    message: &'static str,
) -> Result<(), CompatError> {
    if object.get(key).is_some_and(has_meaningful_value) {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            message,
        ));
    }
    Ok(())
}

pub(crate) fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}

pub(super) fn reject_unsupported_root_fields(
    object: &Map<String, Value>,
) -> Result<(), CompatError> {
    reject_present(
        object,
        "background",
        "background mode is not supported for chat-native endpoints",
    )?;
    reject_present(
        object,
        "audio",
        "audio input/output is not supported for chat-native endpoints",
    )?;
    reject_present(
        object,
        "truncation",
        "truncation controls are not supported for chat-native endpoints",
    )?;
    reject_mutually_exclusive_state_fields(object)?;
    reject_reasoning_config(object)?;
    reject_text_config(object)?;
    reject_unknown_root_fields(object)
}

fn reject_mutually_exclusive_state_fields(object: &Map<String, Value>) -> Result<(), CompatError> {
    if object.get("conversation").is_some_and(has_meaningful_value)
        && object
            .get("previous_response_id")
            .is_some_and(has_meaningful_value)
    {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "conversation and previous_response_id cannot be used together",
        ));
    }
    Ok(())
}

fn reject_unknown_root_fields(object: &Map<String, Value>) -> Result<(), CompatError> {
    const SUPPORTED_FIELDS: &[&str] = &[
        "input",
        "include",
        "instructions",
        "max_output_tokens",
        "model",
        "parallel_tool_calls",
        "prompt_cache_key",
        "reasoning",
        "conversation",
        "previous_response_id",
        "stream",
        "temperature",
        "text",
        "tool_choice",
        "tools",
        "top_p",
    ];

    for (key, value) in object {
        if SUPPORTED_FIELDS.contains(&key.as_str()) || !has_meaningful_value(value) {
            continue;
        }
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("responses field `{key}` is not supported for chat-native endpoints"),
        ));
    }
    Ok(())
}

fn reject_text_config(object: &Map<String, Value>) -> Result<(), CompatError> {
    let Some(text) = object.get("text") else {
        return Ok(());
    };
    let text_object = text.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses text config must be an object for chat-native endpoints",
        )
    })?;
    for (key, value) in text_object {
        if key == "format" || !has_meaningful_value(value) {
            continue;
        }
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("responses text.{key} is not supported for chat-native endpoints"),
        ));
    }
    let format = text_object
        .get("format")
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses text.format is required for chat-native endpoints",
            )
        })?
        .as_object()
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses text.format must be an object for chat-native endpoints",
            )
        })?;

    let format_type = format
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses text.format.type is required for chat-native endpoints",
            )
        })?;

    match format_type {
        "text" => reject_unknown_fields(
            format,
            &["type"],
            "responses text.format",
            "chat-native endpoints",
        ),
        "json_object" => reject_unknown_fields(
            format,
            &["type"],
            "responses text.format",
            "chat-native endpoints",
        ),
        "json_schema" => {
            let name = format
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "responses text.format.name is required for json_schema chat-native endpoints",
                    )
                })?;
            if format.get("schema").is_none() {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "responses text.format.schema is required for json_schema chat-native endpoints",
                ));
            }
            if !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                || name.len() > 64
            {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "responses text.format.name must use only letters, numbers, underscores, or dashes and be at most 64 characters",
                ));
            }
            reject_unknown_fields(
                format,
                &["description", "name", "schema", "strict", "type"],
                "responses text.format",
                "chat-native endpoints",
            )
        }
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "responses text.format type `{other}` is not supported for chat-native endpoints"
            ),
        )),
    }
}

fn reject_reasoning_config(object: &Map<String, Value>) -> Result<(), CompatError> {
    let Some(reasoning) = object.get("reasoning") else {
        return Ok(());
    };
    let reasoning_object = reasoning.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses reasoning config must be an object for chat-native endpoints",
        )
    })?;

    let allowed_efforts = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
    if let Some(effort) = reasoning_object
        .get("effort")
        .filter(|value| has_meaningful_value(value))
    {
        let effort = effort.as_str().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
            || {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "responses reasoning.effort must be a non-empty string for chat-native endpoints",
                )
            },
        )?;
        if !allowed_efforts.contains(&effort) {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                format!(
                    "responses reasoning.effort `{effort}` is not supported for chat-native endpoints"
                ),
            ));
        }
    }

    reject_unknown_fields(
        reasoning_object,
        &["effort"],
        "responses reasoning",
        "chat-native endpoints",
    )
}

pub(crate) fn translate_reasoning(value: &Value) -> Result<Option<Value>, CompatError> {
    let reasoning = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses reasoning config must be an object for chat-native endpoints",
        )
    })?;
    Ok(reasoning
        .get("effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|effort| Value::String(effort.to_string())))
}

pub(crate) fn translate_text_format(value: &Value) -> Result<Value, CompatError> {
    let text = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses text config must be an object for chat-native endpoints",
        )
    })?;
    let format = text
        .get("format")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses text.format must be an object for chat-native endpoints",
            )
        })?;
    let format_type = format
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses text.format.type is required for chat-native endpoints",
            )
        })?;

    match format_type {
        "text" => Ok(serde_json::json!({ "type": "text" })),
        "json_object" => Ok(serde_json::json!({ "type": "json_object" })),
        "json_schema" => {
            let mut schema = Map::new();
            for field in ["name", "description", "schema", "strict"] {
                if let Some(value) = format.get(field) {
                    schema.insert(field.to_string(), value.clone());
                }
            }
            Ok(serde_json::json!({
                "type": "json_schema",
                "json_schema": Value::Object(schema),
            }))
        }
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "responses text.format type `{other}` is not supported for chat-native endpoints"
            ),
        )),
    }
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    prefix: &str,
    suffix: &str,
) -> Result<(), CompatError> {
    for (key, value) in object {
        if allowed.contains(&key.as_str()) || !has_meaningful_value(value) {
            continue;
        }
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("{prefix}.{key} is not supported for {suffix}"),
        ));
    }
    Ok(())
}
