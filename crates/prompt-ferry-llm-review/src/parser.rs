use anyhow::{Context, anyhow};
use serde_json::Value;

use super::{ReviewDecision, ReviewResult};

pub fn parse_review_completion_body(body: &[u8]) -> anyhow::Result<ReviewResult> {
    let value = serde_json::from_slice::<Value>(body).context("invalid review response json")?;
    let content = extract_completion_text(&value)
        .ok_or_else(|| anyhow!("review response did not include message content"))?;
    let json_text = extract_json_object(&content)
        .ok_or_else(|| anyhow!("review response did not include a json object"))?;
    parse_review_json(&json_text)
}

fn parse_review_json(text: &str) -> anyhow::Result<ReviewResult> {
    let value = serde_json::from_str::<Value>(text).context("invalid review decision json")?;
    let decision = match value
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
    {
        "allow" => ReviewDecision::Allow,
        "flag" => ReviewDecision::Flag,
        other => return Err(anyhow!("unsupported review decision `{other}`")),
    };
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let categories = match value.get("categories") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) if !value.trim().is_empty() => vec![value.trim().to_string()],
        _ => Vec::new(),
    };
    Ok(ReviewResult {
        decision,
        reason,
        categories,
    })
}

fn extract_completion_text(value: &Value) -> Option<String> {
    let content = value.pointer("/choices/0/message/content")?;
    let text = match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .or_else(|| item.get("content"))
                    .or_else(|| item.get("input_text"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    (!text.trim().is_empty()).then_some(text)
}

fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut start = None;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => {
                if start.is_none() {
                    start = Some(index);
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    let begin = start?;
                    return Some(text[begin..=index].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_review_json_from_string_content() {
        let body = br#"{"choices":[{"message":{"content":"{\"decision\":\"flag\",\"reason\":\"needs review\",\"categories\":[\"security\"]}"}}]}"#;
        let parsed = parse_review_completion_body(body).unwrap();
        assert_eq!(parsed.decision, ReviewDecision::Flag);
        assert_eq!(parsed.reason, "needs review");
        assert_eq!(parsed.categories, vec!["security"]);
    }

    #[test]
    fn extracts_json_object_from_wrapped_text() {
        let body = br#"{"choices":[{"message":{"content":"decision follows:\n```json\n{\"decision\":\"allow\",\"reason\":\"ok\",\"categories\":[]}\n```"}}]}"#;
        let parsed = parse_review_completion_body(body).unwrap();
        assert_eq!(parsed.decision, ReviewDecision::Allow);
    }
}
