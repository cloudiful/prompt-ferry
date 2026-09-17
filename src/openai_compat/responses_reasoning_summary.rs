use serde_json::{Value, json};

use super::extract_text;
use super::responses_reasoning_echo::{
    minimax_reasoning_encrypted_token, mint_reasoning_encrypted_content,
};

/// Fill missing reasoning summaries and, when `mint_minimax_encrypted_content`
/// is set (MiniMax Responses passthrough), mint `minimax-<original id>` echo
/// tokens on reasoning items lacking `encrypted_content` (issue #459).
/// Non-reasoning and already-tokenized items are untouched, and a body with
/// nothing to change is returned byte-for-byte.
pub(crate) fn normalize_responses_reasoning_body(
    body: Vec<u8>,
    mint_minimax_encrypted_content: bool,
) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let changed = if mint_minimax_encrypted_content {
        normalize_response_reasoning_items(&mut value, true)
    } else {
        normalize_responses_reasoning_summaries(&mut value)
    };
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

fn normalize_response_reasoning_items(value: &mut Value, mint: bool) -> bool {
    if value.get("response").is_some() {
        let Some(response) = value.get_mut("response") else {
            return false;
        };
        normalize_response_reasoning_output(response, mint)
    } else {
        normalize_response_reasoning_output(value, mint)
    }
}

fn normalize_response_reasoning_output(value: &mut Value, mint: bool) -> bool {
    let Some(output) = value.get_mut("output").and_then(Value::as_array_mut) else {
        return false;
    };
    output.iter_mut().fold(false, |changed, item| {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            return changed;
        }
        let minted = mint
            && item
                .get("id")
                .and_then(Value::as_str)
                .map(minimax_reasoning_encrypted_token)
                .is_some_and(|token| mint_reasoning_encrypted_content(item, &token));
        ensure_reasoning_summary(item, None) || minted || changed
    })
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
        assert_eq!(
            normalize_responses_reasoning_body(body.clone(), false),
            body
        );
    }

    #[test]
    fn mints_minimax_echo_token_when_missing() {
        let body = json!({
            "output": [{
                "id": "resp_1_rs",
                "type": "reasoning",
                "summary": [],
                "content": [{"type": "reasoning_text", "text": "think"}]
            }]
        });
        let out = normalize_responses_reasoning_body(serde_json::to_vec(&body).unwrap(), true);
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["output"][0]["encrypted_content"], "minimax-resp_1_rs");
        // Summary is still synthesized from the plaintext content.
        assert_eq!(value["output"][0]["summary"][0]["text"], "think");
    }

    #[test]
    fn mint_is_borrowed_when_encrypted_content_already_present() {
        let body = br#"{"output":[{"id":"rs_1","type":"reasoning","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"t"}]}]}"#;
        let out = normalize_responses_reasoning_body(body.to_vec(), true);
        assert_eq!(out, body);
    }

    #[test]
    fn mint_does_not_touch_items_without_id() {
        let body =
            br#"{"output":[{"type":"reasoning","summary":[{"type":"summary_text","text":"t"}]}]}"#;
        let out = normalize_responses_reasoning_body(body.to_vec(), true);
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert!(value["output"][0].get("encrypted_content").is_none());
    }
}
