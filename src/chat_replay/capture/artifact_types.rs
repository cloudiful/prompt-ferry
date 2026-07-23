use serde_json::Value;

use crate::stream_text::Utf8LineDecoder;

#[derive(Debug, Clone)]
pub struct AssistantArtifact {
    pub message_json: Value,
    pub has_reasoning_content: bool,
    pub has_tool_calls: bool,
}

#[derive(Debug, Default)]
pub struct AssistantArtifactCapture {
    pub(super) is_sse: bool,
    pub(super) sse_decoder: Utf8LineDecoder,
    pub(super) sse_decode_failed: bool,
    pub(super) json_body: Vec<u8>,
    pub(super) json_body_truncated: bool,
    pub(super) stream_message: StreamAssistantMessage,
    pub(super) finalized_message: Option<Value>,
}

#[derive(Debug, Default)]
pub struct ResponsesArtifactCapture {
    pub(super) is_sse: bool,
    pub(super) sse_decoder: Utf8LineDecoder,
    pub(super) sse_decode_failed: bool,
    pub(super) json_body: Vec<u8>,
    pub(super) json_body_truncated: bool,
    pub(super) sse_events: Vec<Value>,
    pub(super) finalized_output: Option<Vec<Value>>,
}

#[derive(Debug, Default)]
pub(super) struct StreamAssistantMessage {
    pub(super) content: String,
    pub(super) refusal: String,
    pub(super) reasoning_content: String,
    pub(super) phase: Option<String>,
    pub(super) tool_calls: Vec<StreamToolCallState>,
    pub(super) active_tool_call_positions: std::collections::BTreeMap<usize, usize>,
}

#[derive(Debug, Default)]
pub(super) struct StreamToolCallState {
    pub(super) id: Option<String>,
    pub(super) name: Option<String>,
    pub(super) arguments: String,
}
