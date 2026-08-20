use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
}

pub struct UsageCapture {
    is_sse: bool,
    pub model: Option<String>,
    pub response_id: Option<String>,
    pub provider_conversation_key: Option<String>,
    pub usage: TokenUsage,
    pub response_text: String,
    pub response_text_truncated: bool,
    max_response_text_capture_bytes: usize,
    sse_decoder: Utf8LineDecoder,
    sse_decode_failed: bool,
    json_body: Vec<u8>,
    json_body_truncated: bool,
    saw_output: bool,
}

impl UsageCapture {
    pub fn new(is_sse: bool, model: Option<String>) -> Self {
        Self {
            is_sse,
            model,
            response_id: None,
            provider_conversation_key: None,
            usage: TokenUsage::default(),
            response_text: String::new(),
            response_text_truncated: false,
            max_response_text_capture_bytes: 1024 * 1024,
            sse_decoder: Utf8LineDecoder::default(),
            sse_decode_failed: false,
            json_body: Vec::new(),
            json_body_truncated: false,
            saw_output: false,
        }
    }

    pub fn set_response_text_capture_limit(&mut self, limit: usize) {
        self.max_response_text_capture_bytes = limit.max(1);
        self.truncate_response_text();
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) -> bool {
        let had_output = self.saw_output;
        if self.is_sse {
            self.observe_sse_chunk(chunk);
        } else if !self.json_body_truncated {
            const MAX_JSON_CAPTURE: usize = 1024 * 1024;
            if self.json_body.len().saturating_add(chunk.len()) <= MAX_JSON_CAPTURE {
                self.json_body.extend_from_slice(chunk);
            } else {
                self.json_body.clear();
                self.json_body_truncated = true;
            }
        }
        !had_output && self.saw_output
    }

    pub fn finish(&mut self) -> bool {
        let had_output = self.saw_output;
        if self.is_sse {
            if self.sse_decode_failed {
                return false;
            }
            if let Ok(Some(line)) = self.sse_decoder.finish() {
                self.observe_sse_line(&line);
            }
        } else if !self.json_body_truncated
            && let Ok(value) = serde_json::from_slice::<Value>(&self.json_body)
        {
            self.observe_json_value(&value);
        }
        !had_output && self.saw_output
    }

    fn observe_sse_chunk(&mut self, chunk: &[u8]) {
        if self.sse_decode_failed {
            return;
        }
        let lines = match self.sse_decoder.push(chunk) {
            Ok(lines) => lines,
            Err(_) => {
                self.sse_decode_failed = true;
                return;
            }
        };
        for line in lines {
            self.observe_sse_line(&line);
        }
    }

    fn observe_sse_line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            self.observe_json_value(&value);
        }
    }

    fn observe_json_value(&mut self, value: &Value) {
        let payload = value
            .get("response")
            .or_else(|| value.get("message"))
            .unwrap_or(value);
        if self.model.is_none() {
            self.model = payload
                .get("model")
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.response_id.is_none() {
            self.response_id = payload
                .get("id")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.provider_conversation_key.is_none() {
            self.provider_conversation_key = payload
                .get("conversation")
                .or_else(|| value.get("conversation"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(usage) = extract_usage(payload).or_else(|| extract_usage(value)) {
            self.usage = merge_usage(&self.usage, &usage);
        }
        if value.get("response").is_some() {
            text::append_text(&mut self.response_text, &text::extract_output_text(payload));
        } else {
            text::append_text(&mut self.response_text, &text::extract_output_text(value));
        }
        let saw_output = output_events::has_output_event(value)
            || (value.get("type").is_none() && output_events::has_output_event(payload));
        if saw_output {
            self.saw_output = true;
        }
        self.truncate_response_text();
    }

    fn truncate_response_text(&mut self) {
        if self.response_text.len() <= self.max_response_text_capture_bytes {
            return;
        }
        let mut end = self.max_response_text_capture_bytes;
        while !self.response_text.is_char_boundary(end) {
            end -= 1;
        }
        self.response_text.truncate(end);
        self.response_text_truncated = true;
    }
}

fn merge_usage(current: &TokenUsage, next: &TokenUsage) -> TokenUsage {
    let input_tokens = next.input_tokens.or(current.input_tokens);
    let output_tokens = next.output_tokens.or(current.output_tokens);
    TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens: next.total_tokens.or(current.total_tokens).or_else(|| {
            input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input + output)
        }),
        cached_tokens: next.cached_tokens.or(current.cached_tokens),
        cache_read_tokens: next.cache_read_tokens.or(current.cache_read_tokens),
        cache_write_tokens: next.cache_write_tokens.or(current.cache_write_tokens),
    }
}

#[cfg(test)]
mod tests {
    use super::UsageCapture;

    #[test]
    fn ttft_starts_on_reasoning_or_tool_output_not_lifecycle_events() {
        let mut capture = UsageCapture::new(true, None);

        assert!(!capture.observe_chunk(b"data: {\"type\":\"response.created\"}\n\n"));
        assert!(capture.observe_chunk(
            b"data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"think\"}\n\n"
        ));
        assert!(!capture.observe_chunk(
            b"data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n"
        ));
    }

    #[test]
    fn ttft_handles_an_output_event_split_across_sse_chunks() {
        let mut capture = UsageCapture::new(true, None);

        assert!(!capture.observe_chunk(
            b"data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{"
        ));
        assert!(capture.observe_chunk(b"\"}\n\n"));
    }

    #[test]
    fn captures_anthropic_stream_text_and_usage() {
        let mut capture = UsageCapture::new(true, None);
        assert!(!capture.observe_chunk(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":3}}}\n\n"
        ));
        assert!(capture.observe_chunk(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n"
        ));
        capture.observe_chunk(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4}}\n\n",
        );
        capture.finish();
        assert_eq!(capture.response_id.as_deref(), Some("msg_1"));
        assert_eq!(capture.response_text, "hello");
        assert_eq!(capture.usage.input_tokens, Some(15));
        assert_eq!(capture.usage.output_tokens, Some(4));
        assert_eq!(capture.usage.cache_read_tokens, Some(3));
    }

    #[test]
    fn sse_merge_replaces_zero_early_usage_with_final_anthropic_usage() {
        let mut capture = UsageCapture::new(true, None);
        // First, an Anthropic message_start chunk reports empty (zero) input and zero cache.
        capture.observe_chunk(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0}}}\n\n",
        );
        // Then a final message_delta provides the authoritative Anthropic usage.
        capture.observe_chunk(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":176,\"output_tokens\":42,\"cache_read_input_tokens\":82793,\"cache_creation_input_tokens\":7}}\n\n",
        );
        capture.finish();

        // Canonical total = ordinary (176) + cache_read (82793) + cache_write (7).
        assert_eq!(capture.usage.input_tokens, Some(82976));
        assert_eq!(capture.usage.output_tokens, Some(42));
        assert_eq!(capture.usage.cache_read_tokens, Some(82793));
        assert_eq!(capture.usage.cache_write_tokens, Some(7));
        assert_eq!(capture.usage.total_tokens, Some(83018));
    }

    #[test]
    fn sse_merge_keeps_earlier_cache_when_later_chunk_lacks_cache_fields() {
        let mut capture = UsageCapture::new(true, None);
        capture.observe_chunk(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_3\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":3,\"cache_creation_input_tokens\":2}}}\n\n",
        );
        // A late chunk that only carries an output_tokens update should not lose
        // the cache meters observed in earlier usage.
        capture.observe_chunk(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4}}\n\n",
        );
        capture.finish();

        assert_eq!(capture.usage.input_tokens, Some(17));
        assert_eq!(capture.usage.output_tokens, Some(4));
        assert_eq!(capture.usage.cache_read_tokens, Some(3));
        assert_eq!(capture.usage.cache_write_tokens, Some(2));
        assert_eq!(capture.usage.total_tokens, Some(21));
    }

    #[test]
    fn sse_merge_does_not_double_count_cached_tokens_for_openai_responses() {
        let mut capture = UsageCapture::new(true, None);
        capture
            .observe_chunk(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n");
        capture.observe_chunk(
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":120,\"output_tokens\":20,\"total_tokens\":140,\"input_tokens_details\":{\"cached_tokens\":30,\"cache_read_tokens\":30,\"cache_write_tokens\":7}}}}\n\n",
        );
        capture.finish();

        // OpenAI Responses: input_tokens already includes cache and cache_write.
        // The canonical total must remain 120 (no Anthropic-style fold).
        assert_eq!(capture.usage.input_tokens, Some(120));
        assert_eq!(capture.usage.cache_read_tokens, Some(30));
        assert_eq!(capture.usage.cache_write_tokens, Some(7));
        assert_eq!(capture.usage.cached_tokens, Some(30));
    }
}
