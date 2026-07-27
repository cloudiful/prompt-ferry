use http::StatusCode;
use serde_json::{Value, json};

use crate::openai_compat::{
    CompatError,
    response_items::{
        ChatToolCallDelta, ToolCallArgumentRepairStatus, build_response_object,
        default_response_usage, extract_chat_delta_reasoning_text, extract_chat_delta_text,
        extract_chat_delta_tool_calls, function_call_item, generate_call_id, generate_message_id,
        generate_reasoning_id, generate_response_id, message_item_with_status,
        normalize_tool_call_arguments, reasoning_item_with_status, response_shell,
        usage_from_chat_value,
    },
    response_stream_state::{StreamToolCallState, sse_event},
};
use crate::stream_text::Utf8LineDecoder;

mod responses_to_chat;
mod stream_adapter;
mod stream_tool_calls;

pub mod anthropic_stream_adapter;

pub use anthropic_stream_adapter::AnthropicResponseStreamAdapter;
pub use responses_to_chat::ResponsesChatResponseStreamAdapter;
pub use stream_adapter::ChatResponseStreamAdapter;

pub(super) fn decode_sse_chunk(
    decoder: &mut Utf8LineDecoder,
    chunk: &[u8],
) -> Result<Vec<String>, CompatError> {
    decoder.push(chunk).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "upstream SSE stream is not valid UTF-8",
        )
    })
}

pub(super) fn finish_sse_decoder(
    decoder: &mut Utf8LineDecoder,
) -> Result<Option<String>, CompatError> {
    decoder.finish().map_err(|_| {
        CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "upstream SSE stream ended with incomplete UTF-8",
        )
    })
}
