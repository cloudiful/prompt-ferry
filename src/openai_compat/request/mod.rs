use http::StatusCode;
use serde_json::{Map, Value};

use crate::openai_compat::{CompatError, NormalizedResponsesRequest};

mod request_parse;
mod request_tests;
mod request_translate;
mod request_validate;

pub use request_parse::{conversation_key, is_streaming_request, previous_response_id};
pub use request_translate::responses_request_to_chat;
pub(crate) use request_validate::{
    has_meaningful_value, translate_reasoning, translate_text_format,
};
