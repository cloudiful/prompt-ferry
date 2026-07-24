mod responses_error;
mod responses_input_normalize;
mod responses_output;
mod responses_request_state;

use std::collections::{HashMap, HashSet};

use http::StatusCode;
use serde_json::{Map, Value, json};

use crate::openai_compat::request::has_meaningful_value;
use crate::openai_compat::request_content::translate_content;
use crate::openai_compat::request_input::translate_input;
use crate::openai_compat::request_tools::{translate_tool_choice, translate_tools};
use crate::openai_compat::response_items::{chat_output_items_from_message, extract_text};
use crate::openai_compat::{CompatError, request};

use responses_input_normalize::{
    ItemKind, input_items_from_object, invalid_continuation, item_kind,
    normalize_instruction_messages, normalize_responses_input_for_upstream, required_string_field,
};

pub(crate) use responses_error::normalize_response_error;
pub(crate) use responses_input_normalize::output_items_to_input_items;
pub(crate) use responses_output::{
    assistant_message_to_output_items, extract_output_items_from_responses_value,
    output_items_to_assistant_message, persisted_artifact, persisted_assistant_message,
    persisted_output_items, responses_stream_output_items,
};
pub(crate) use responses_request_state::{
    NormalizedResponsesRequest, validate_raw_responses_request_body,
};
