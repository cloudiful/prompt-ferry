use super::{conversation_key, previous_response_id, request_parse, request_validate};
use crate::openai_compat::{CompatError, NormalizedResponsesRequest};
#[cfg(test)]
use serde_json::Value;

pub fn responses_request_to_chat(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let object = request_parse::parse_request_object(body)?;
    request_validate::reject_unsupported_root_fields(&object)?;
    let request = NormalizedResponsesRequest::from_body(body)?;
    request.validate_for_chat_compat(
        &std::collections::HashSet::new(),
        previous_response_id(body).is_some() || conversation_key(body).is_some(),
    )?;
    request.to_chat_request_with_prefix(&[])
}

/// Stateless Responses→Chat translation for chat-native upstreams.
///
/// Rejects `previous_response_id`/`conversation` with
/// `invalid_responses_continuation`; accepts and drops
/// `reasoning.summary="auto"` and `include` (including
/// `reasoning.encrypted_content`, never echoed back); forwards
/// `reasoning.effort` (including `xhigh`) as `reasoning_effort`.
/// Item references and orphan `function_call_output` reuse the existing
/// `validate_for_chat_compat` continuation checks with no replay context.
pub fn responses_stateless_request_to_chat(body: &[u8]) -> Result<Vec<u8>, CompatError> {
    let object = request_parse::parse_request_object(body)?;
    request_validate::reject_unsupported_root_fields_for_stateless(&object)?;
    let request = NormalizedResponsesRequest::from_body(body)?;
    request.validate_for_chat_compat(&std::collections::HashSet::new(), false)?;
    request.to_chat_request_with_prefix(&[])
}

#[cfg(test)]
pub fn responses_request_to_chat_with_prefix(
    body: &[u8],
    prefix_messages: &[Value],
) -> Result<Vec<u8>, CompatError> {
    let object = request_parse::parse_request_object(body)?;
    request_validate::reject_unsupported_root_fields(&object)?;
    let request = NormalizedResponsesRequest::from_body(body)?;
    request.validate_for_chat_compat(
        &request_parse::prefix_tool_call_ids(prefix_messages),
        previous_response_id(body).is_some() || conversation_key(body).is_some(),
    )?;
    request.to_chat_request_with_prefix(prefix_messages)
}
