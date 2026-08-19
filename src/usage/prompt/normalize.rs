use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{
    NormalizedPromptRequest, PROMPT_PREVIEW_TEXT_LIMIT, PromptBlockSeed, fingerprint_prompt_refs,
    prompt_message_refs,
};

pub fn normalize_prompt_request(path: &str, body: &[u8]) -> Option<NormalizedPromptRequest> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    match path {
        "/v1/chat/completions" => normalize_chat_request(&value),
        "/v1/responses" => normalize_responses_request(&value),
        "/v1/messages" => normalize_anthropic_request(&value),
        _ => None,
    }
}

fn normalize_chat_request(value: &Value) -> Option<NormalizedPromptRequest> {
    let messages = value.get("messages")?.as_array()?;
    let mut items = Vec::new();
    for message in messages {
        let Value::Object(object) = message else {
            continue;
        };
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("message")
            .to_string();
        let mut content_object = object.clone();
        content_object.remove("role");
        let content_json = canonicalize_value(&Value::Object(content_object));
        let preview_text = prompt_preview_text(&role, &content_json);
        items.push(PromptBlockSeed {
            role,
            content_json,
            preview_text,
        });
    }
    if items.is_empty() {
        return None;
    }
    normalized_request(items, None, None)
}

fn normalize_responses_request(value: &Value) -> Option<NormalizedPromptRequest> {
    let mut items = Vec::new();
    if let Some(instructions) = value
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let content_json = canonicalize_value(&serde_json::json!({
            "role": "system",
            "content": instructions,
        }));
        let preview_text = prompt_preview_text("system", &content_json);
        items.push(PromptBlockSeed {
            role: "system".to_string(),
            content_json,
            preview_text,
        });
    }
    let input = value.get("input")?;
    let input_items = match input {
        Value::Array(items) => items.clone(),
        item => vec![item.clone()],
    };
    for item in input_items {
        let role = item
            .get("role")
            .and_then(Value::as_str)
            .or_else(|| item.get("type").and_then(Value::as_str))
            .unwrap_or("input")
            .to_string();
        let content_json = canonicalize_value(&item);
        let preview_text = prompt_preview_text(&role, &content_json);
        items.push(PromptBlockSeed {
            role,
            content_json,
            preview_text,
        });
    }
    if items.is_empty() {
        return None;
    }
    normalized_request(
        items,
        value
            .get("previous_response_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        value
            .get("conversation")
            .and_then(Value::as_str)
            .map(str::to_string),
    )
}

fn normalize_anthropic_request(value: &Value) -> Option<NormalizedPromptRequest> {
    let mut items = Vec::new();
    if let Some(system) = value.get("system") {
        let content_json = canonicalize_value(system);
        let preview_text = prompt_preview_text("system", &content_json);
        items.push(PromptBlockSeed {
            role: "system".to_string(),
            content_json,
            preview_text,
        });
    }
    for message in value.get("messages")?.as_array()? {
        let Value::Object(object) = message else {
            continue;
        };
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("message")
            .to_string();
        let content_json =
            canonicalize_value(&object.get("content").cloned().unwrap_or(Value::Null));
        let preview_text = prompt_preview_text(&role, &content_json);
        items.push(PromptBlockSeed {
            role,
            content_json,
            preview_text,
        });
    }
    (!items.is_empty())
        .then(|| normalized_request(items, None, None))
        .flatten()
}

fn normalized_request(
    items: Vec<PromptBlockSeed>,
    previous_response_id: Option<String>,
    conversation: Option<String>,
) -> Option<NormalizedPromptRequest> {
    let refs = prompt_message_refs(&items);
    let normalized_bytes_len = serde_json::to_vec(&refs).ok()?.len();
    let fingerprint = fingerprint_prompt_refs(&refs);
    Some(NormalizedPromptRequest {
        items,
        previous_response_id,
        conversation,
        normalized_bytes_len,
        fingerprint,
    })
}

fn prompt_preview_text(role: &str, content_json: &Value) -> String {
    let text = extract_text(content_json);
    let text = text.trim();
    if !text.is_empty() {
        return text.to_string();
    }
    if content_json.as_object().is_some_and(Map::is_empty) {
        role.to_string()
    } else {
        json_preview(content_json)
    }
}

fn extract_text(value: &Value) -> String {
    let mut out = String::new();
    extract_text_into(value, &mut out);
    out
}

fn extract_text_into(value: &Value, out: &mut String) {
    if out.chars().count() >= PROMPT_PREVIEW_TEXT_LIMIT {
        return;
    }
    match value {
        Value::String(text) => push_preview_text(out, text),
        Value::Array(items) => {
            for item in items {
                push_separator(out);
                extract_text_into(item, out);
            }
        }
        Value::Object(object) => {
            if let Some(value) = object
                .get("text")
                .or_else(|| object.get("content"))
                .or_else(|| object.get("input_text"))
                .or_else(|| object.get("output_text"))
            {
                extract_text_into(value, out);
                return;
            }
            for value in object.values() {
                push_separator(out);
                extract_text_into(value, out);
            }
        }
        _ => {}
    }
}

fn push_separator(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') && out.chars().count() < PROMPT_PREVIEW_TEXT_LIMIT {
        out.push('\n');
    }
}

fn push_preview_text(out: &mut String, text: &str) {
    let remaining = PROMPT_PREVIEW_TEXT_LIMIT.saturating_sub(out.chars().count());
    out.extend(text.chars().take(remaining));
}

fn json_preview(value: &Value) -> String {
    let mut out = String::new();
    push_json_preview(value, &mut out);
    out
}

fn push_json_preview(value: &Value, out: &mut String) {
    if out.chars().count() >= PROMPT_PREVIEW_TEXT_LIMIT {
        return;
    }
    match value {
        Value::Null => push_preview_text(out, "null"),
        Value::Bool(value) => push_preview_text(out, if *value { "true" } else { "false" }),
        Value::Number(value) => push_preview_text(out, &value.to_string()),
        Value::String(value) => {
            push_preview_text(out, "\"");
            push_preview_text(out, value);
            push_preview_text(out, "\"");
        }
        Value::Array(items) => {
            push_preview_text(out, "[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    push_preview_text(out, ",");
                }
                push_json_preview(item, out);
            }
            push_preview_text(out, "]");
        }
        Value::Object(object) => {
            push_preview_text(out, "{");
            for (index, (key, value)) in object.iter().enumerate() {
                if index > 0 {
                    push_preview_text(out, ",");
                }
                push_preview_text(out, "\"");
                push_preview_text(out, key);
                push_preview_text(out, "\":");
                push_json_preview(value, out);
            }
            push_preview_text(out, "}");
        }
    }
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_value(value)))
                .collect::<BTreeMap<_, _>>();
            let map = sorted
                .into_iter()
                .fold(Map::new(), |mut acc, (key, value)| {
                    acc.insert(key, value);
                    acc
                });
            Value::Object(map)
        }
        _ => value.clone(),
    }
}
