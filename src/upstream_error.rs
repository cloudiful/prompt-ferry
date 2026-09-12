use serde_json::Value;

pub(crate) fn is_quota_exhaustion(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    normalized.contains("insufficient_quota")
        || normalized.contains("insufficient quota")
        // DeepSeek returns HTTP 402 with `{"error":{"message":"Insufficient
        // Balance"}}`, which carries no quota keyword of its own (issue #287).
        || normalized.contains("insufficient balance")
        || normalized.contains("quota")
        || normalized.contains("credit")
        || normalized.contains("billing")
        || normalized.contains("token plan")
        || normalized.contains("token_plan")
        || normalized.contains("usage limit")
        || normalized.contains("usage_limit")
        // OpenCode Go returns HTTP 429 with a `GoUsageLimitError` envelope
        // (`Monthly usage limit reached` / `Free usage limit`). The typed code
        // is the stable signal when the human message changes (issue #310).
        || normalized.contains("gousagelimiterror")
        || normalized.contains("free usage limit")
        || normalized.contains("monthly usage")
        || text.contains("用量上限")
        || text.contains("额度")
        || contains_provider_code_2056(text, &normalized)
}

fn contains_provider_code_2056(text: &str, normalized: &str) -> bool {
    let structured_code = serde_json::from_str::<Value>(text)
        .ok()
        .is_some_and(|value| contains_structured_provider_code(&value));
    structured_code
        || normalized.contains("(2056)")
        || normalized.contains("code: 2056")
        || normalized.contains("code=2056")
        || normalized.contains("code 2056")
}

fn contains_structured_provider_code(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            let normalized_key = key.to_ascii_lowercase().replace(['-', '_'], " ");
            (matches!(
                normalized_key.trim(),
                "code" | "error code" | "provider code" | "provider error code"
            ) && scalar_is_2056(value))
                || contains_structured_provider_code(value)
        }),
        Value::Array(values) => values.iter().any(contains_structured_provider_code),
        _ => false,
    }
}

fn scalar_is_2056(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_i64() == Some(2056),
        Value::String(value) => value.trim() == "2056",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_quota_exhaustion;

    #[test]
    fn recognizes_quota_markers_across_provider_formats() {
        for body in [
            "insufficient_quota",
            "Token Plan usage limit exhausted",
            "用量上限已用尽",
            "当前额度不足",
            r#"{"error":{"code":2056,"message":"provider limit"}}"#,
            "provider code: 2056",
            // DeepSeek 402: the raw upstream body has no quota keyword.
            r#"{"error":{"message":"Insufficient Balance","type":"unknown_error"}}"#,
        ] {
            assert!(is_quota_exhaustion(body), "expected quota marker in {body}");
        }
    }

    #[test]
    fn recognizes_the_opencode_go_usage_limit_envelope() {
        // Real `request_records` body (issue #310): the typed code and the
        // monthly message must both classify as quota exhaustion, and the
        // typed code alone must be enough if the message is reworded.
        let body = r#"{"type":"error","error":{"type":"GoUsageLimitError","message":"Monthly usage limit reached. Resets in 4 days."},"metadata":{"limitName":"monthly"}}"#;
        assert!(is_quota_exhaustion(body));
        assert!(is_quota_exhaustion("GoUsageLimitError"));
        assert!(is_quota_exhaustion("Free usage limit reached"));
    }

    #[test]
    fn does_not_classify_an_unrelated_server_error() {
        assert!(!is_quota_exhaustion(
            r#"{"error":{"message":"temporary provider failure"}}"#
        ));
    }
}
