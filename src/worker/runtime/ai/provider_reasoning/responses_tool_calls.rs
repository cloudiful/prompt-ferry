use super::super::replay::{ReplayFailureKind, ResponsesReplayToolCall, replay_unavailable};
use serde_json::{Value, json};

const CLIENT_EXECUTED_TOOL_MARKER: &str = "The following tool was executed by the user";

pub(super) fn response_tool_calls(
    input: &[Value],
) -> Result<Vec<ResponsesReplayToolCall>, crate::openai_compat::CompatError> {
    let mut calls = Vec::new();
    for (input_index, item) in input.iter().enumerate() {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        let call_id = response_tool_call_id(item)?;
        let name = item.get("name").and_then(Value::as_str).ok_or_else(|| {
            replay_unavailable(
                ReplayFailureKind::InvalidHistory,
                "Responses function_call item has no function name",
            )
        })?;
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        calls.push(ResponsesReplayToolCall {
            input_index,
            client_executed: is_client_executed_tool_call(input, input_index, &call_id),
            call_id: call_id.clone(),
            tool_call: json!({
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments,
                }
            }),
        });
    }
    Ok(calls)
}

pub(super) fn replayable_tool_calls(
    input: &[Value],
    calls: &[ResponsesReplayToolCall],
) -> Vec<ResponsesReplayToolCall> {
    calls
        .iter()
        .filter(|call| !call.client_executed && call_needs_reasoning(input, call.input_index))
        .cloned()
        .collect()
}

fn response_tool_call_id(item: &Value) -> Result<String, crate::openai_compat::CompatError> {
    let object = item.as_object().ok_or_else(|| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "Responses function_call item must be a JSON object",
        )
    })?;
    let call_id = object
        .get("call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|call_id| !call_id.is_empty())
        .ok_or_else(|| {
            replay_unavailable(
                ReplayFailureKind::InvalidHistory,
                "Responses function_call item has no call_id",
            )
        })?
        .to_string();
    Ok(call_id)
}

fn is_client_executed_tool_call(input: &[Value], input_index: usize, call_id: &str) -> bool {
    let Some(output_index) = input_index.checked_add(1) else {
        return false;
    };
    let Some(output) = input.get(output_index) else {
        return false;
    };
    if output.get("type").and_then(Value::as_str) != Some("function_call_output")
        || output.get("call_id").and_then(Value::as_str).map(str::trim) != Some(call_id)
    {
        return false;
    }

    let mut marker_index = input_index;
    while let Some(previous_index) = marker_index.checked_sub(1) {
        marker_index = previous_index;
        let item = &input[marker_index];
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") | Some("function_call_output") => continue,
            _ => return has_client_executed_tool_marker(item),
        }
    }
    false
}

fn has_client_executed_tool_marker(item: &Value) -> bool {
    if item.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    match item.get("content") {
        Some(Value::String(content)) => has_client_executed_tool_marker_text(content),
        Some(Value::Array(parts)) => parts.iter().any(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_some_and(has_client_executed_tool_marker_text)
        }),
        _ => false,
    }
}

fn has_client_executed_tool_marker_text(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim() == CLIENT_EXECUTED_TOOL_MARKER)
}

pub(super) fn call_needs_reasoning(input: &[Value], input_index: usize) -> bool {
    let mut first_index = input_index;
    while first_index > 0
        && input[first_index - 1].get("type").and_then(Value::as_str) == Some("function_call")
    {
        first_index -= 1;
    }
    !has_reasoning_for_group(input, first_index)
}

fn has_reasoning_for_group(input: &[Value], first_index: usize) -> bool {
    let mut index = first_index;
    while index > 0 {
        let item = &input[index - 1];
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            break;
        }
        if reasoning_content(item).is_some() {
            return true;
        }
        index -= 1;
    }
    false
}

fn reasoning_content(item: &Value) -> Option<&Value> {
    item.get("content")
        .and_then(Value::as_array)
        .and_then(|parts| {
            parts.iter().find(|part| {
                part.get("type").and_then(Value::as_str) == Some("reasoning_text")
                    && part
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })
        })
}
