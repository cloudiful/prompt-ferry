use serde_json::{Value, json};

use super::extract_text;

pub(crate) fn normalize_responses_reasoning_summaries_body(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    if !normalize_responses_reasoning_summaries(&mut value) {
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
    output
        .iter_mut()
        .map(|item| ensure_reasoning_summary(item, None))
        .any(|changed| changed)
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_responses_reasoning_summaries, normalize_responses_reasoning_summaries_body,
    };
    use serde_json::json;

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
        assert_eq!(
            normalize_responses_reasoning_summaries_body(body.clone()),
            body
        );
    }
}
