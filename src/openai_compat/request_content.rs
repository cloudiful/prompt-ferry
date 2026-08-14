use http::StatusCode;
use serde_json::{Value, json};

use super::CompatError;

pub(crate) struct TranslatedAssistantContent {
    pub(crate) content: Value,
    pub(crate) reasoning_content: Option<String>,
}

pub(crate) fn translate_content(value: &Value) -> Result<Value, CompatError> {
    match value {
        Value::String(text) => Ok(Value::String(text.clone())),
        Value::Array(parts) => Ok(Value::Array(translate_parts(parts)?)),
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("message") {
                return translate_wrapped_message_content(object);
            }
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return Ok(Value::String(text.to_string()));
            }
            if object.get("type").is_some() {
                return Ok(Value::Array(
                    translate_part(value)?.into_iter().collect::<Vec<_>>(),
                ));
            }
            Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses content object must be a supported text/image part",
            ))
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses content must be a string or supported part array",
        )),
    }
}

pub(crate) fn translate_assistant_content(
    value: &Value,
) -> Result<TranslatedAssistantContent, CompatError> {
    match value {
        Value::Null => Ok(TranslatedAssistantContent {
            content: Value::Null,
            reasoning_content: None,
        }),
        Value::String(text) => Ok(TranslatedAssistantContent {
            content: Value::String(text.clone()),
            reasoning_content: None,
        }),
        Value::Array(parts) => assistant_content_from_parts(parts),
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("message") {
                return translate_wrapped_assistant_message_content(object);
            }
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return Ok(TranslatedAssistantContent {
                    content: Value::String(text.to_string()),
                    reasoning_content: None,
                });
            }
            if object.get("type").is_some() {
                return match translate_assistant_part(value)? {
                    AssistantPart::Content(part) => Ok(TranslatedAssistantContent {
                        content: Value::Array(vec![part]),
                        reasoning_content: None,
                    }),
                    AssistantPart::Reasoning(text) => Ok(TranslatedAssistantContent {
                        content: Value::Null,
                        reasoning_content: (!text.trim().is_empty()).then_some(text),
                    }),
                    AssistantPart::Ignored => Ok(TranslatedAssistantContent {
                        content: Value::Null,
                        reasoning_content: None,
                    }),
                };
            }
            Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "assistant content object must be a supported text/reasoning part",
            ))
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "assistant content must be null, a string, or supported part array",
        )),
    }
}

pub(crate) fn translate_tool_output_content(value: &Value) -> Result<Value, CompatError> {
    match value {
        Value::String(text) => Ok(Value::String(text.clone())),
        Value::Array(parts) => Ok(Value::Array(
            parts
                .iter()
                .map(translate_tool_output_part)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return Ok(Value::String(text.to_string()));
            }
            if object.get("type").is_some() {
                return Ok(Value::Array(vec![translate_tool_output_part(value)?]));
            }
            Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "function_call_output content must be text or supported text parts",
            ))
        }
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "function_call_output content must be a string or supported part array",
        )),
    }
}

fn translate_parts(parts: &[Value]) -> Result<Vec<Value>, CompatError> {
    let mut translated = Vec::new();
    for part in parts {
        translated.extend(translate_content_part(part)?);
    }
    Ok(translated)
}

fn translate_part(value: &Value) -> Result<Option<Value>, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses content parts must be JSON objects",
        )
    })?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("input_text");
    match part_type {
        "input_text" | "text" | "output_text" => {
            let text = object
                .get("text")
                .or_else(|| object.get("input_text"))
                .or_else(|| object.get("output_text"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "text parts require a text field",
                    )
                })?;
            Ok(Some(json!({
                "type": "text",
                "text": text,
            })))
        }
        // Chat-compatible multimodal providers use the standard image_url part;
        // pass URLs through without fetching or rewriting them.
        "input_image" => {
            let image_url = match object.get("image_url") {
                Some(Value::String(url)) => json!({ "url": url }),
                Some(Value::Object(image)) => {
                    if image.get("file_id").is_some() {
                        return Err(CompatError::new(
                            StatusCode::BAD_REQUEST,
                            "unsupported_feature",
                            "input_image file_id is not supported for chat-native endpoints",
                        ));
                    }
                    if image.get("url").is_none() {
                        return Err(CompatError::new(
                            StatusCode::BAD_REQUEST,
                            "unsupported_feature",
                            "image_url objects require a url field",
                        ));
                    }
                    Value::Object(image.clone())
                }
                _ => {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "image parts require image_url",
                    ));
                }
            };
            Ok(Some(json!({
                "type": "image_url",
                "image_url": image_url,
            })))
        }
        "item_reference" => Ok(None),
        // Responses reasoning items are metadata, not Chat content. Bare
        // items are handled by the tool-call translator when they precede a
        // function call; other occurrences are ignored for Chat upstreams.
        "reasoning" => Ok(None),
        "input_file" => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "input_file is not supported for chat-native endpoints",
        )),
        "input_audio" | "audio" | "output_audio" => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "audio input/output is not supported for chat-native endpoints",
        )),
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "responses content part type `{other}` is not supported for chat-native endpoints"
            ),
        )),
    }
}

fn translate_content_part(value: &Value) -> Result<Vec<Value>, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "responses content parts must be JSON objects",
        )
    })?;
    if object.get("type").and_then(Value::as_str) == Some("message") {
        return content_value_to_parts(&translate_wrapped_message_content(object)?);
    }
    Ok(translate_part(value)?.into_iter().collect())
}

enum AssistantPart {
    Content(Value),
    Reasoning(String),
    Ignored,
}

fn assistant_content_from_parts(
    parts: &[Value],
) -> Result<TranslatedAssistantContent, CompatError> {
    let mut content_parts = Vec::new();
    let mut reasoning_content = String::new();
    for part in parts {
        for translated in translate_assistant_parts(part)? {
            match translated {
                AssistantPart::Content(part) => content_parts.push(part),
                AssistantPart::Reasoning(text) => reasoning_content.push_str(&text),
                AssistantPart::Ignored => {}
            }
        }
    }
    Ok(TranslatedAssistantContent {
        content: if content_parts.is_empty() {
            Value::Null
        } else {
            Value::Array(content_parts)
        },
        reasoning_content: (!reasoning_content.trim().is_empty()).then_some(reasoning_content),
    })
}

fn translate_assistant_parts(value: &Value) -> Result<Vec<AssistantPart>, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "assistant content parts must be JSON objects",
        )
    })?;
    if object.get("type").and_then(Value::as_str) == Some("message") {
        let translated = translate_wrapped_assistant_message_content(object)?;
        let mut parts = content_value_to_parts(&translated.content)?
            .into_iter()
            .map(AssistantPart::Content)
            .collect::<Vec<_>>();
        if let Some(reasoning_content) = translated.reasoning_content {
            parts.push(AssistantPart::Reasoning(reasoning_content));
        }
        return Ok(parts);
    }
    Ok(vec![translate_assistant_part(value)?])
}

fn translate_assistant_part(value: &Value) -> Result<AssistantPart, CompatError> {
    let object = value.as_object().ok_or_else(|| {
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
        "reasoning" => Ok(AssistantPart::Reasoning(extract_reasoning_text(object)?)),
        _ => match translate_part(value)? {
            Some(part) => Ok(AssistantPart::Content(part)),
            None => Ok(AssistantPart::Ignored),
        },
    }
}

fn translate_wrapped_message_content(
    object: &serde_json::Map<String, Value>,
) -> Result<Value, CompatError> {
    let content = object.get("content").ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "message parts require content",
        )
    })?;
    translate_content(content)
}

fn translate_wrapped_assistant_message_content(
    object: &serde_json::Map<String, Value>,
) -> Result<TranslatedAssistantContent, CompatError> {
    let content = object.get("content").ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "assistant message parts require content",
        )
    })?;
    translate_assistant_content(content)
}

fn content_value_to_parts(value: &Value) -> Result<Vec<Value>, CompatError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({
            "type": "text",
            "text": text,
        })]),
        Value::Array(parts) => Ok(parts.clone()),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "message parts must contain text, null, or supported content parts",
        )),
    }
}

fn extract_reasoning_text(object: &serde_json::Map<String, Value>) -> Result<String, CompatError> {
    let content = object.get("content").ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "reasoning parts require a content array",
        )
    })?;
    let parts = content.as_array().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "reasoning part content must be an array",
        )
    })?;

    let mut text = String::new();
    for part in parts {
        let part = part.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "reasoning part content entries must be JSON objects",
            )
        })?;
        let part_type = part
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("reasoning_text");
        match part_type {
            "reasoning_text" => {
                let piece = part.get("text").and_then(Value::as_str).ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "reasoning_text parts require a text field",
                    )
                })?;
                text.push_str(piece);
            }
            other => {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    format!(
                        "reasoning content part type `{other}` is not supported for chat-native endpoints"
                    ),
                ));
            }
        }
    }
    Ok(text)
}

fn translate_tool_output_part(value: &Value) -> Result<Value, CompatError> {
    let object = value.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "function_call_output content parts must be JSON objects",
        )
    })?;
    let part_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("input_text");
    match part_type {
        "input_text" | "text" | "output_text" => {
            let text = object
                .get("text")
                .or_else(|| object.get("input_text"))
                .or_else(|| object.get("output_text"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "function_call_output text parts require a text field",
                    )
                })?;
            Ok(json!({
                "type": "text",
                "text": text,
            }))
        }
        "input_image" | "input_file" => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "function_call_output content part type `{part_type}` is not supported for chat-native endpoints"
            ),
        )),
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!(
                "function_call_output content part type `{other}` is not supported for chat-native endpoints"
            ),
        )),
    }
}
