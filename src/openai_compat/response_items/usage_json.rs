use super::*;

pub(crate) fn usage_from_chat_value(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?;
    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_i64);
    let total_tokens = usage.get("total_tokens").and_then(Value::as_i64);
    let cached_tokens = usage
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64);

    Some(json!({
        "input_tokens": prompt_tokens,
        "output_tokens": completion_tokens,
        "total_tokens": total_tokens.or_else(|| Some(prompt_tokens.unwrap_or_default() + completion_tokens.unwrap_or_default())),
        "input_tokens_details": if let Some(cached_tokens) = cached_tokens {
            json!({ "cached_tokens": cached_tokens })
        } else {
            json!({ "cached_tokens": 0 })
        },
        "output_tokens_details": {
            "reasoning_tokens": usage
                .get("completion_tokens_details")
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or_default()
        },
    }))
}

pub(crate) fn default_response_usage() -> Value {
    json!({
        "input_tokens": 0,
        "input_tokens_details": {
            "cached_tokens": 0
        },
        "output_tokens": 0,
        "output_tokens_details": {
            "reasoning_tokens": 0
        },
        "total_tokens": 0
    })
}
