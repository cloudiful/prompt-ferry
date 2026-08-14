use http::StatusCode;
use serde_json::{Map, Value};

use crate::openai_compat::CompatError;

mod chat_native;
mod chat_to_responses;
mod request_parse;
mod request_tests;
mod request_translate;
mod request_validate;

pub(crate) use chat_native::normalize_chat_request_for_native;
pub use chat_to_responses::chat_request_to_responses;
pub use request_parse::{conversation_key, is_streaming_request, previous_response_id};
pub use request_translate::responses_request_to_chat;
pub(crate) use request_validate::{
    has_meaningful_value, translate_reasoning, translate_text_format,
};
