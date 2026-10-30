use serde_json::{Value, json};

use super::extract_text;
/// Fill missing reasoning summaries while preserving unchanged bodies byte-for-byte.
pub(crate) fn normalize_responses_reasoning_body(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let changed = normalize_responses_reasoning_summaries(&mut value);
    if !changed {
        return body;
    }
    serde_json::to_vec(&value).unwrap_or(body)
}

fn normalize_responses_reasoning_summaries(value: &mut Value) -> bool {
    if let Some(response) = value.get_mut("response") {
        normalize_response_object(response)
    } else {
        normalize_response_object(value)
    }
}

pub(crate) fn ensure_reasoning_summary(item: &mut Value, fallback_text: Option<&str>) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    let has_summary = item
        .get("summary")
        .is_some_and(|summary| !extract_text(summary).trim().is_empty());
    if has_summary {
        return false;
    }
    let text = item
        .get("content")
        .map(extract_text)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            fallback_text
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string)
        });
    let Some(text) = text else {
        return false;
    };
    item["summary"] = json!([{"type": "summary_text", "text": text}]);
    true
}

fn normalize_response_object(value: &mut Value) -> bool {
    let Some(output) = value.get_mut("output").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for item in output.iter_mut() {
        if ensure_reasoning_summary(item, None) {
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::{normalize_responses_reasoning_body, normalize_responses_reasoning_summaries};
    use serde_json::{Value, json};

    #[test]
    fn copies_reasoning_content_into_missing_summary() {
        let mut value = json!({
            "output": [{
                "type": "reasoning",
                "content": [{"type": "reasoning_text", "text": "think"}]
            }]
        });

        assert!(normalize_responses_reasoning_summaries(&mut value));
        assert_eq!(value["output"][0]["summary"][0]["text"], "think");
    }

    #[test]
    fn preserves_existing_summary() {
        let mut value = json!({
            "output": [{
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "short"}],
                "content": [{"type": "reasoning_text", "text": "complete"}]
            }]
        });

        assert!(!normalize_responses_reasoning_summaries(&mut value));
        assert_eq!(value["output"][0]["summary"][0]["text"], "short");
    }

    #[test]
    fn leaves_non_json_bodies_unchanged() {
        let body = b"not json".to_vec();
        assert_eq!(normalize_responses_reasoning_body(body.clone()), body);
    }

    #[test]
    fn normalizes_summary_without_fabricating_encrypted_content() {
        let body = json!({
            "output": [{
                "id": "resp_1_rs",
                "type": "reasoning",
                "summary": [],
                "content": [{"type": "reasoning_text", "text": "think"}]
            }]
        });
        let out = normalize_responses_reasoning_body(serde_json::to_vec(&body).unwrap());
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert!(value["output"][0].get("encrypted_content").is_none());
        assert_eq!(value["output"][0]["summary"][0]["text"], "think");
    }

    #[test]
    fn unchanged_opaque_encrypted_content_is_borrowed() {
        let body = br#"{"output":[{"id":"rs_1","type":"reasoning","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"t"}]}]}"#;
        let out = normalize_responses_reasoning_body(body.to_vec());
        assert_eq!(out, body);
    }

    #[test]
    fn normalizing_summary_preserves_upstream_encrypted_values() {
        for encrypted in [Value::Null, json!(""), json!("  "), json!("opaque-value")] {
            let body = json!({"output":[{"type":"reasoning", "encrypted_content":encrypted, "content":[{"type":"reasoning_text","text":"think"}]}]});
            let result = normalize_responses_reasoning_body(serde_json::to_vec(&body).unwrap());
            let result: Value = serde_json::from_slice(&result).unwrap();
            assert_eq!(result["output"][0]["encrypted_content"], encrypted);
            assert_eq!(result["output"][0]["summary"][0]["text"], "think");
        }
    }

    #[test]
    fn unchanged_reasoning_without_id_has_no_encrypted_content() {
        let body =
            br#"{"output":[{"type":"reasoning","summary":[{"type":"summary_text","text":"t"}]}]}"#;
        let out = normalize_responses_reasoning_body(body.to_vec());
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert!(value["output"][0].get("encrypted_content").is_none());
    }
}
