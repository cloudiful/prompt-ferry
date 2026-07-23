use super::*;

pub(super) fn parse_request_object(body: &[u8]) -> Result<Map<String, Value>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "request body must be valid JSON",
        )
    })?;
    value.as_object().cloned().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "responses request must be a JSON object",
        )
    })
}

#[cfg(test)]
pub(super) fn prefix_tool_call_ids(prefix_messages: &[Value]) -> std::collections::HashSet<String> {
    prefix_messages
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|message| message.get("tool_calls").and_then(Value::as_array))
        .flat_map(|tool_calls| tool_calls.iter())
        .filter_map(Value::as_object)
        .filter_map(|tool_call| tool_call.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

pub fn previous_response_id(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("previous_response_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

pub fn conversation_key(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("conversation")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

pub fn is_streaming_request(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.get("stream").and_then(Value::as_bool))
        == Some(true)
}
