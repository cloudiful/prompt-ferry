use serde_json::Value;

use crate::openai_compat::{
    extract_output_items_from_responses_value, persisted_artifact, responses_stream_output_items,
};

use super::{
    AssistantArtifact, ResponsesArtifactCapture,
    shared::{finish_json_capture, finish_sse_line, observe_json_chunk},
};

impl ResponsesArtifactCapture {
    pub fn new(is_sse: bool) -> Self {
        Self {
            is_sse,
            ..Self::default()
        }
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        if self.is_sse {
            self.observe_sse_chunk(chunk);
        } else {
            observe_json_chunk(&mut self.json_body, &mut self.json_body_truncated, chunk);
        }
    }

    pub fn finish(&mut self) {
        if self.is_sse {
            if self.sse_decode_failed {
                return;
            }
            if let Some(line) = finish_sse_line(&mut self.sse_decoder) {
                self.observe_sse_line(&line);
            }
            self.finalized_output = responses_stream_output_items(&self.sse_events).ok();
            return;
        }
        if self.json_body_truncated {
            return;
        }
        if let Some(value) = finish_json_capture(&self.json_body) {
            self.finalized_output = extract_output_items_from_responses_value(&value).ok();
        }
    }

    pub fn artifact(&self) -> Option<AssistantArtifact> {
        let output_items = self.finalized_output.clone()?;
        let (message_json, has_reasoning_content, has_tool_calls) =
            persisted_artifact(None, output_items)?;
        Some(AssistantArtifact {
            message_json,
            has_reasoning_content,
            has_tool_calls,
        })
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
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        self.sse_events.push(value);
    }
}
