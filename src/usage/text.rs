use super::*;

pub fn extract_usage(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .or_else(|| Some(input_tokens? + output_tokens?));
    let cached_tokens = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64);
    let cache_read_tokens = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .and_then(|details| {
            details
                .get("cache_read_tokens")
                .or_else(|| details.get("cached_tokens"))
        })
        .and_then(Value::as_i64);
    let cache_write_tokens = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .and_then(|details| details.get("cache_write_tokens"))
        .and_then(Value::as_i64);

    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && cached_tokens.is_none()
        && cache_read_tokens.is_none()
        && cache_write_tokens.is_none()
    {
        return None;
    }

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

pub(super) fn extract_output_text(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(text) = value
        .get("delta")
        .filter(|_| is_visible_delta_event(value))
        .or_else(|| value.get("text"))
        .or_else(|| value.get("output_text"))
        .and_then(Value::as_str)
    {
        parts.push(text.to_string());
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(content) = choice
                .get("delta")
                .or_else(|| choice.get("message"))
                .and_then(|message| message.get("content"))
            {
                let text = value_text(content);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
        }
    }
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    let text = value_text(
                        part.get("text")
                            .or_else(|| part.get("output_text"))
                            .unwrap_or(part),
                    );
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
            }
        }
    }
    parts.join("")
}

pub(super) fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("input_text"))
            .or_else(|| object.get("output_text"))
            .map(value_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(super) fn append_text(target: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    target.push_str(text);
}

pub fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut value = text.chars().take(limit).collect::<String>();
    value.push('…');
    value
}

pub(super) fn has_content(value: &Value) -> bool {
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| {
            event_type == "response.output_text.delta" || event_type == "response.output_text.done"
        })
    {
        return true;
    }
    if value
        .get("delta")
        .filter(|_| is_visible_delta_event(value))
        .and_then(Value::as_str)
        .is_some_and(|delta| !delta.is_empty())
    {
        return true;
    }
    if value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| choices.iter().any(choice_has_content))
    {
        return true;
    }
    value
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(output_item_has_content))
}

fn choice_has_content(choice: &Value) -> bool {
    choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        .is_some()
}

fn is_visible_delta_event(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|event_type| event_type == "response.output_text.delta")
}

fn output_item_has_content(item: &Value) -> bool {
    item.get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|part| {
                part.get("text")
                    .or_else(|| part.get("output_text"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            })
        })
}
