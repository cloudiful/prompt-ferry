mod request;
mod request_content;
mod request_input;
mod request_tools;
mod response;
mod response_items;
mod response_stream;
mod response_stream_state;
#[cfg(test)]
mod response_stream_tests;
#[cfg(test)]
mod response_stream_utf8_tests;
mod responses_state;

use http::StatusCode;

pub(crate) use request::{conversation_key, previous_response_id};
pub use request::{is_streaming_request, responses_request_to_chat};
pub(crate) use request_input::translate_input;
pub use response::chat_response_to_responses;
pub(crate) use response_items::{extract_text, reasoning_details_from_text};
pub use response_stream::{AnthropicResponseStreamAdapter, ChatResponseStreamAdapter};
pub(crate) use response_stream_state::sse_event;
pub(crate) use responses_state::{
    NormalizedResponsesRequest, assistant_message_to_output_items,
    extract_output_items_from_responses_value, normalize_response_error,
    output_items_to_input_items, persisted_artifact, persisted_assistant_message,
    persisted_output_items, responses_stream_output_items, validate_raw_responses_request_body,
};

#[derive(Debug, Clone)]
pub struct CompatError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl CompatError {
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}
