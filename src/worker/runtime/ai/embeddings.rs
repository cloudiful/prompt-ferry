//! Issue #1032: embeddings-specific request helpers.
//!
//! The embeddings protocol is deliberately separate from the Chat/Responses/
//! Anthropic pipelines: it never streams, never translates, and never applies
//! a chat transformation. These helpers keep that surface small and testable.

use serde_json::Value;

/// The text fragments of an embeddings `input` that a redaction policy could
/// rewrite.
///
/// Only text-bearing inputs are returned: a single string, or the string
/// elements of a batch. Token ids, token-id arrays, and any non-text element
/// contribute nothing (there is no text to redact), so a token-only request is
/// never refused for a redaction conflict.
pub(super) fn embedding_text_inputs(body: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(input) = value.get("input") else {
        return Vec::new();
    };
    match input {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::embedding_text_inputs;

    #[test]
    fn collects_single_and_batch_text() {
        assert_eq!(
            embedding_text_inputs(br#"{"input":"hello"}"#),
            vec!["hello".to_string()]
        );
        assert_eq!(
            embedding_text_inputs(br#"{"input":["a","b"]}"#),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn token_inputs_have_no_text() {
        assert!(embedding_text_inputs(br#"{"input":[1,2,3]}"#).is_empty());
        assert!(embedding_text_inputs(br#"{"input":[[1,2],[3]]}"#).is_empty());
        assert!(embedding_text_inputs(br#"{"model":"m"}"#).is_empty());
        assert!(embedding_text_inputs(br#"{"input":null}"#).is_empty());
        assert!(embedding_text_inputs(b"not json").is_empty());
    }
}
