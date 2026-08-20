//! Provider-aware usage parser used by the historical backfill.
//!
//! This module re-uses the canonical `UsageCapture`/`extract_usage` pipeline
//! from `crate::usage` so the backfill cannot drift from new-request parsing.
//! It only adds the small surface needed to drive a one-shot decision:
//! parse a raw retained body and report a structured outcome.

use crate::usage::{TokenUsage, UsageCapture, extract_usage};

/// Parses a single retained response body through the same provider-aware
/// `UsageCapture`/`extract_usage` path used for new requests. Both SSE and JSON
/// response bodies are supported by feeding the same bytes through a fresh
/// capture and calling `finish()` to apply the final state.
pub fn parse_raw_response(raw_response_body: &str) -> Option<TokenUsage> {
    if raw_response_body.is_empty() {
        return None;
    }
    let is_sse = raw_response_body.contains("\ndata:") || raw_response_body.starts_with("data:");
    let mut capture = UsageCapture::new(is_sse, None);
    capture.observe_chunk(raw_response_body.as_bytes());
    capture.finish();
    let usage = capture.usage;
    if usage.input_tokens.is_none()
        && usage.output_tokens.is_none()
        && usage.total_tokens.is_none()
        && usage.cached_tokens.is_none()
        && usage.cache_read_tokens.is_none()
        && usage.cache_write_tokens.is_none()
    {
        // Fallback: SSE payloads where the line decoder sees no `data:` line
        // (event-framed bodies, padded buffers, multi-segment streams split
        // across captures) — recover by parsing the body as a single JSON
        // value or by inspecting the embedded `response` envelope.
        if let Some(value) = serde_json::from_str::<serde_json::Value>(raw_response_body).ok() {
            return extract_usage(&value).or_else(|| value.get("response").and_then(extract_usage));
        }
        return None;
    }
    Some(usage)
}

#[cfg(test)]
mod tests {
    use super::parse_raw_response;

    #[test]
    fn parses_anthropic_sse_into_canonical_anthropic_tokens() {
        let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0}}}\n\n\
            event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":176,\"output_tokens\":42,\"cache_read_input_tokens\":82793,\"cache_creation_input_tokens\":7}}\n\n";
        let usage = parse_raw_response(body).expect("usage parsed");
        assert_eq!(usage.input_tokens, Some(82976));
        assert_eq!(usage.output_tokens, Some(42));
        assert_eq!(usage.cache_read_tokens, Some(82793));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn parses_openai_chat_json_into_canonical_tokens() {
        let body = r#"{"usage":{"prompt_tokens":120,"completion_tokens":20,"total_tokens":140,"prompt_tokens_details":{"cached_tokens":30}}}"#;
        let usage = parse_raw_response(body).expect("usage parsed");
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.total_tokens, Some(140));
    }

    #[test]
    fn parses_openai_responses_sse_into_canonical_tokens() {
        let body = "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":120,\"output_tokens\":20,\"total_tokens\":140,\"input_tokens_details\":{\"cached_tokens\":30,\"cache_write_tokens\":7}}}}\n\n";
        let usage = parse_raw_response(body).expect("usage parsed");
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn parse_returns_none_for_malformed_or_usage_free_payload() {
        assert!(parse_raw_response("not a json body at all").is_none());
        assert!(parse_raw_response("").is_none());
        assert!(parse_raw_response("event: ping\ndata: {\"type\":\"ping\"}\n\n").is_none());
    }

    #[test]
    fn partial_sse_with_only_zero_usage_returns_some_with_zeros() {
        // A partial stream that only carried the message_start zero-filled
        // usage chunk produces `Some(usage)` with every field set to
        // `Some(0)`. The parser still returns a value here because a real
        // zero-token response would also look like this. The process-level
        // truncated guard must reject this case before the parsed value
        // reaches decide_repair.
        let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_partial\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0}}}\n\n";
        let usage = parse_raw_response(body).expect("zero-only usage still returns Some");
        assert_eq!(usage.input_tokens, Some(0));
        assert_eq!(usage.output_tokens, Some(0));
        assert_eq!(usage.cache_read_tokens, Some(0));
        assert_eq!(usage.cache_write_tokens, Some(0));
    }
}
