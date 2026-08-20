use super::*;

pub fn extract_usage(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    let mut input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64);
    let cached_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        });
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(|details| {
                    details
                        .get("cache_read_tokens")
                        .or_else(|| details.get("cached_tokens"))
                })
                .and_then(Value::as_i64)
        });
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64);
    let cache_write_tokens = cache_write_tokens.or_else(|| {
        usage
            .get("input_tokens_details")
            .or_else(|| usage.get("prompt_tokens_details"))
            .and_then(|details| details.get("cache_write_tokens"))
            .and_then(Value::as_i64)
    });
    if usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some()
    {
        let cache_total =
            cache_read_tokens.unwrap_or_default() + cache_write_tokens.unwrap_or_default();
        input_tokens = input_tokens
            .map(|value| value.saturating_add(cache_total))
            .or((cache_total > 0).then_some(cache_total));
    }
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input + output)
        });

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_usage;

    #[test]
    fn extracts_openai_chat_usage_and_cache_read_tokens() {
        let usage = extract_usage(&json!({
            "usage": {
                "prompt_tokens": 120,
                "completion_tokens": 20,
                "total_tokens": 140,
                "prompt_tokens_details": { "cached_tokens": 30 }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, None);
    }

    #[test]
    fn extracts_openai_responses_usage_and_separate_cache_meters() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": 20,
                "input_tokens_details": {
                    "cached_tokens": 30,
                    "cache_read_tokens": 30,
                    "cache_write_tokens": 7
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn returns_unknown_when_provider_has_no_usage_fields() {
        assert!(extract_usage(&json!({ "usage": {} })).is_none());
        assert!(extract_usage(&json!({})).is_none());
    }

    #[test]
    fn extracts_anthropic_cache_usage() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": 20,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 7
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(157));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cached_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }
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
    if let Some(text) = value
        .get("delta")
        .and_then(|delta| delta.get("text").or_else(|| delta.get("thinking")))
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
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for part in content {
            let text = value_text(
                part.get("text")
                    .or_else(|| part.get("thinking"))
                    .unwrap_or(part),
            );
            if !text.is_empty() {
                parts.push(text);
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

fn is_visible_delta_event(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|event_type| event_type == "response.output_text.delta")
}
