use super::*;

pub(super) fn chat_content_to_responses_parts(
    content: &Value,
    assistant: bool,
) -> Result<Vec<Value>, CompatError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({
            "type": if assistant { "output_text" } else { "input_text" },
            "text": text,
        })]),
        Value::Array(parts) => parts
            .iter()
            .map(|part| chat_part_to_responses(part, assistant))
            .collect(),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat message content must be a string, null, or an array of text/image parts",
        )),
    }
}

fn chat_part_to_responses(part: &Value, assistant: bool) -> Result<Value, CompatError> {
    let object = part.as_object().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "chat content parts must be JSON objects",
        )
    })?;
    match object.get("type").and_then(Value::as_str).unwrap_or("text") {
        "text" | "input_text" | "output_text" => {
            let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "chat text parts require a text field",
                )
            })?;
            Ok(json!({
                "type": if assistant { "output_text" } else { "input_text" },
                "text": text,
            }))
        }
        "image_url" => {
            let image = object.get("image_url").ok_or_else(|| {
                CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "chat image_url parts require image_url",
                )
            })?;
            let (image_url, detail) = match image {
                Value::String(url) => (url.clone(), None),
                Value::Object(image) => {
                    let url = image.get("url").and_then(Value::as_str).ok_or_else(|| {
                        CompatError::new(
                            StatusCode::BAD_REQUEST,
                            "unsupported_feature",
                            "chat image_url objects require a url field",
                        )
                    })?;
                    (url.to_string(), image.get("detail").cloned())
                }
                _ => {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "chat image_url must be a URL string or object",
                    ));
                }
            };
            let mut translated = Map::new();
            translated.insert("type".to_string(), Value::String("input_image".to_string()));
            translated.insert("image_url".to_string(), Value::String(image_url));
            if let Some(detail) = detail {
                translated.insert("detail".to_string(), detail);
            }
            Ok(Value::Object(translated))
        }
        other => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            format!("chat content part type `{other}` is not supported for Responses"),
        )),
    }
}

pub(super) fn chat_content_to_text(content: &Value) -> Result<String, CompatError> {
    match content {
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => parts
            .iter()
            .map(|part| {
                let object = part.as_object().ok_or_else(|| {
                    CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "system/developer content parts must be text objects",
                    )
                })?;
                if object.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        "system/developer messages cannot contain images",
                    ));
                }
                object
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        CompatError::new(
                            StatusCode::BAD_REQUEST,
                            "unsupported_feature",
                            "system/developer text parts require a text field",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        Value::Null => Ok(String::new()),
        _ => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_feature",
            "system/developer content must be text",
        )),
    }
}
