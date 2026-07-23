use http::StatusCode;
use serde_json::{Value, json};

use crate::openai_compat::CompatError;

mod ids;
mod messages;
mod text_extract;
mod tool_call_repair;
mod tool_calls;
mod usage_json;

pub(crate) use ids::generate_reasoning_id;
pub(crate) use ids::{generate_call_id, generate_message_id, generate_response_id};
pub(crate) use messages::{
    build_response_object, chat_output_items_from_message, chat_output_items_from_response,
    function_call_item, message_item_with_status, reasoning_item_with_status, response_shell,
};
pub(crate) use text_extract::{
    extract_chat_delta_reasoning_text, extract_chat_delta_text, extract_reasoning_text,
    extract_text, reasoning_details_from_text,
};
pub(crate) use tool_call_repair::{ToolCallArgumentRepairStatus, normalize_tool_call_arguments};
pub(crate) use tool_calls::{ChatToolCallDelta, extract_chat_delta_tool_calls};
pub(crate) use usage_json::{default_response_usage, usage_from_chat_value};
