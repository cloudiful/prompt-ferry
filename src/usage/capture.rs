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
    saw_content: bool,
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
            saw_content: false,
        }
    }

    pub fn set_response_text_capture_limit(&mut self, limit: usize) {
        self.max_response_text_capture_bytes = limit.max(1);
        self.truncate_response_text();
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) -> bool {
        let had_content = self.saw_content;
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
        !had_content && self.saw_content
    }

    pub fn finish(&mut self) {
        if self.is_sse {
            if self.sse_decode_failed {
                return;
            }
            if let Ok(Some(line)) = self.sse_decoder.finish() {
                self.observe_sse_line(&line);
            }
        } else if !self.json_body_truncated
            && let Ok(value) = serde_json::from_slice::<Value>(&self.json_body)
        {
            self.observe_json_value(&value);
        }
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
        let payload = value.get("response").unwrap_or(value);
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
            self.usage = usage;
        }
        if value.get("response").is_some() {
            text::append_text(&mut self.response_text, &text::extract_output_text(payload));
        } else {
            text::append_text(&mut self.response_text, &text::extract_output_text(value));
        }
        if text::has_content(payload) || text::has_content(value) {
            self.saw_content = true;
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
