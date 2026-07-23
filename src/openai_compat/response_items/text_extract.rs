use super::*;

pub(crate) fn extract_reasoning_text(value: &Value) -> String {
    match value {
        Value::Object(object) => object
            .get("reasoning_content")
            .or_else(|| object.get("reasoning_details"))
            .map(extract_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(crate) fn reasoning_details_from_text(text: &str) -> Value {
    Value::Array(vec![json!({
        "text": text,
    })])
}

pub(crate) fn extract_chat_delta_text(value: &Value) -> String {
    value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| choice.get("delta").and_then(|delta| delta.get("content")))
        .map(extract_text)
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn extract_chat_delta_reasoning_text(value: &Value) -> String {
    value
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| choice.get("delta"))
        .map(extract_reasoning_text)
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(extract_text).collect::<Vec<_>>().join(""),
        Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("output_text"))
            .map(extract_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(super) fn output_text_from_items(items: &[Value]) -> String {
    items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content"))
        .map(extract_text)
        .collect::<Vec<_>>()
        .join("")
}
