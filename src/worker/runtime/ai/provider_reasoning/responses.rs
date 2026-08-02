use super::replay::{
    ReplayFailureKind, load_tool_call_replay_state, replay_unavailable, resolve_replay_parents,
    targets_deepseek,
};
use crate::{
    db,
    openai_compat::{persisted_assistant_message, persisted_output_items},
    worker_admin::AdminState,
};
use serde_json::{Value, json};

pub(crate) async fn restore_responses_reasoning(
    admin_state: Option<&AdminState>,
    user_id: Option<i64>,
    route: &db::RouteConfig,
    conversation_id: Option<uuid::Uuid>,
    parent_event_id: Option<i64>,
    request_body: &[u8],
) -> Result<Option<Vec<u8>>, crate::openai_compat::CompatError> {
    let Ok(mut value) = serde_json::from_slice::<Value>(request_body) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    if !targets_deepseek(route, model.as_deref()) {
        return Ok(None);
    }
    let Some(input) = object.get_mut("input").and_then(Value::as_array_mut) else {
        return Ok(None);
    };

    let groups = response_tool_call_groups(input)?;
    let groups = groups
        .into_iter()
        .filter(|group| !has_reasoning_for_group(input, group.first_index))
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Ok(None);
    }

    let messages = groups
        .iter()
        .map(|group| group.message.clone())
        .collect::<Vec<_>>();
    let requested_call_ids = groups
        .iter()
        .flat_map(|group| group.call_ids.iter().cloned())
        .collect::<Vec<_>>();
    let state = admin_state.ok_or_else(|| {
        replay_unavailable(
            ReplayFailureKind::MissingArtifact,
            "cannot restore Responses reasoning without stored replay state",
        )
    })?;
    let endpoint_id = Some(route.route_id).filter(|id| !id.is_nil());
    let (candidates_by_call_id, artifacts_by_event_id) = load_tool_call_replay_state(
        state,
        &requested_call_ids,
        user_id,
        endpoint_id,
        conversation_id,
        parent_event_id,
    )
    .await?;
    let parents = resolve_replay_parents(
        &messages,
        &candidates_by_call_id,
        &artifacts_by_event_id,
        conversation_id.is_some() || parent_event_id.is_some(),
    )?;

    let mut restored = Vec::with_capacity(groups.len());
    for (index, group) in groups.iter().enumerate() {
        let parent_event_id = parents.get(&index).copied().ok_or_else(|| {
            replay_unavailable(
                ReplayFailureKind::AmbiguousParent,
                "Responses tool-call history has no safely resolved replay parent",
            )
        })?;
        let artifact = artifacts_by_event_id.get(&parent_event_id).ok_or_else(|| {
            replay_unavailable(
                ReplayFailureKind::MissingArtifact,
                "stored replay state is missing a Responses assistant tool-call artifact",
            )
        })?;
        restored.push((group.first_index, reasoning_input_item(artifact)?));
    }

    for (index, reasoning) in restored.into_iter().rev() {
        input.insert(index, reasoning);
    }
    serde_json::to_vec(&value).map(Some).map_err(|_| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "failed to encode the restored Responses request",
        )
    })
}

struct ResponseToolCallGroup {
    first_index: usize,
    call_ids: Vec<String>,
    message: Value,
}

fn response_tool_call_groups(
    input: &[Value],
) -> Result<Vec<ResponseToolCallGroup>, crate::openai_compat::CompatError> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut first_index = None;
    for (index, item) in input.iter().enumerate() {
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            if first_index.is_none() {
                first_index = Some(index);
            }
            current.push(response_tool_call(item)?);
            continue;
        }
        if let Some(first_index) = first_index.take() {
            groups.push(build_group(first_index, std::mem::take(&mut current)));
        }
    }
    if let Some(first_index) = first_index {
        groups.push(build_group(first_index, current));
    }
    Ok(groups)
}

fn response_tool_call(item: &Value) -> Result<(String, Value), crate::openai_compat::CompatError> {
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
    let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "Responses function_call item has no function name",
        )
    })?;
    let arguments = object
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok((
        call_id.clone(),
        json!({
            "id": call_id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments,
            }
        }),
    ))
}

fn build_group(first_index: usize, tool_calls: Vec<(String, Value)>) -> ResponseToolCallGroup {
    let call_ids = tool_calls
        .iter()
        .map(|(call_id, _)| call_id.clone())
        .collect();
    let tool_calls: Vec<Value> = tool_calls
        .into_iter()
        .map(|(_, tool_call)| tool_call)
        .collect();
    ResponseToolCallGroup {
        first_index,
        call_ids,
        message: json!({
            "role": "assistant",
            "content": null,
            "tool_calls": tool_calls,
        }),
    }
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

fn reasoning_input_item(artifact: &Value) -> Result<Value, crate::openai_compat::CompatError> {
    let mut content = Vec::new();
    for item in persisted_output_items(artifact).map_err(|_| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "stored Responses assistant artifact is invalid",
        )
    })? {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        if let Some(parts) = item.get("content").and_then(Value::as_array) {
            for part in parts {
                if part.get("type").and_then(Value::as_str) != Some("reasoning_text") {
                    continue;
                }
                let Some(text) = part.get("text").and_then(Value::as_str) else {
                    continue;
                };
                if !text.trim().is_empty() {
                    content.push(json!({"type": "reasoning_text", "text": text}));
                }
            }
        }
    }
    if content.is_empty()
        && let Ok(message) = persisted_assistant_message(artifact)
        && let Some(reasoning) = message.get("reasoning_content")
    {
        let text = crate::openai_compat::extract_text(reasoning);
        if !text.trim().is_empty() {
            content.push(json!({"type": "reasoning_text", "text": text}));
        }
    }
    if content.is_empty() {
        return Err(replay_unavailable(
            ReplayFailureKind::MissingReasoning,
            "stored Responses assistant tool-call turn is missing reasoning_text",
        ));
    }
    Ok(json!({
        "type": "reasoning",
        "content": content,
    }))
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

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
