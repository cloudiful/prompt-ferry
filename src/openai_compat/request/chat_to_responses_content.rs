use sha2::{Digest, Sha256};

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

pub(super) fn chat_reasoning_to_responses_item(message: &Map<String, Value>) -> Option<Value> {
    let text = message
        .get("reasoning_content")
        .map(crate::openai_compat::extract_text)
        .unwrap_or_default();
    if text.trim().is_empty() {
        return None;
    }
    Some(json!({
        "id": deterministic_reasoning_id(&text),
        "type": "reasoning",
        "status": "completed",
        "summary": [],
        "content": [{
            "type": "reasoning_text",
            "text": text,
        }],
    }))
}

/// Responses replay the whole conversation on every turn, so a per-turn random
/// `rs_` id rewrote the replayed prefix and broke upstream prompt caching. The
/// id is therefore derived from the reasoning text itself: the same reasoning
/// block keeps the same id across turns.
fn deterministic_reasoning_id(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut id = String::from("rs_");
    for byte in digest.iter().take(8) {
        id.push_str(&format!("{byte:02x}"));
    }
    id
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

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};
    use sha2::{Digest, Sha256};

    use super::{chat_reasoning_to_responses_item, deterministic_reasoning_id};

    fn reasoning_message(text: &str) -> Map<String, Value> {
        json!({"role": "assistant", "reasoning_content": text})
            .as_object()
            .expect("message object")
            .clone()
    }

    #[test]
    fn reasoning_item_id_is_deterministic_for_the_same_text() {
        let first = chat_reasoning_to_responses_item(&reasoning_message("plan then answer"))
            .expect("first item");
        let replayed = chat_reasoning_to_responses_item(&reasoning_message("plan then answer"))
            .expect("replayed item");

        assert_eq!(
            first["id"], replayed["id"],
            "a replayed reasoning block must keep its id"
        );
        assert_eq!(
            first["id"],
            json!(deterministic_reasoning_id("plan then answer"))
        );
    }

    #[test]
    fn reasoning_item_id_is_derived_from_the_text_digest() {
        let digest = Sha256::digest("plan then answer".as_bytes());
        let expected = format!(
            "rs_{}",
            digest
                .iter()
                .take(8)
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );

        let id = deterministic_reasoning_id("plan then answer");

        assert_eq!(id, expected);
        assert_eq!(id.len(), 19);
        assert!(id.starts_with("rs_"));
        assert!(
            id[3..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }

    #[test]
    fn reasoning_item_ids_differ_for_different_text() {
        let first = chat_reasoning_to_responses_item(&reasoning_message("first thought"))
            .expect("first item");
        let second = chat_reasoning_to_responses_item(&reasoning_message("second thought"))
            .expect("second item");

        assert_ne!(first["id"], second["id"]);
    }

    #[test]
    fn blank_or_absent_reasoning_content_has_no_item() {
        assert!(chat_reasoning_to_responses_item(&reasoning_message("   ")).is_none());
        assert!(chat_reasoning_to_responses_item(&Map::new()).is_none());
    }
}
