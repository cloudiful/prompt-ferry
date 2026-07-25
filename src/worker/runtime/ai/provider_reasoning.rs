use crate::{db, openai_compat::persisted_assistant_message, worker_admin::AdminState};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

pub(super) async fn restore_provider_reasoning(
    admin_state: Option<&AdminState>,
    user_id: Option<i64>,
    route: &db::RouteConfig,
    request_body: &[u8],
) -> Result<Option<Vec<u8>>, crate::openai_compat::CompatError> {
    let Ok(mut value) = serde_json::from_slice::<Value>(request_body) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    let model = object.get("model").and_then(Value::as_str);
    if !targets_reasoning_provider(route, model) {
        return Ok(None);
    }
    let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) else {
        return Ok(None);
    };
    let requested_call_ids = assistant_tool_call_ids(messages)?;
    if requested_call_ids.is_empty() {
        return Ok(None);
    }

    let state = admin_state.ok_or_else(|| {
        replay_unavailable(
            "cannot restore reasoning for assistant tool calls without stored replay state",
        )
    })?;
    let endpoint_id = Some(route.route_id).filter(|id| !id.is_nil());
    let records = db::find_request_record_tool_calls_by_call_ids(
        &state.pool,
        &requested_call_ids,
        user_id,
        endpoint_id,
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "failed to load provider tool-call replay records");
        replay_unavailable("stored tool-call replay records could not be loaded")
    })?;

    let mut parent_by_call_id = HashMap::with_capacity(records.len());
    for record in records {
        if parent_by_call_id
            .insert(record.call_id.clone(), record.parent_event_id)
            .is_some()
        {
            return Err(replay_unavailable(format!(
                "tool call `{}` does not resolve to a unique replay parent",
                record.call_id
            )));
        }
    }
    let requested_call_id_set = requested_call_ids.iter().cloned().collect::<HashSet<_>>();
    if parent_by_call_id.len() != requested_call_id_set.len()
        || parent_by_call_id
            .keys()
            .any(|call_id| !requested_call_id_set.contains(call_id))
    {
        return Err(replay_unavailable(
            "stored replay state is missing an assistant tool-call record",
        ));
    }

    let parent_event_ids = parent_by_call_id.values().copied().collect::<HashSet<_>>();
    let artifacts = db::get_usage_assistant_artifacts(
        &state.pool,
        &parent_event_ids.iter().copied().collect::<Vec<_>>(),
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "failed to load provider assistant replay artifacts");
        replay_unavailable("stored assistant replay artifacts could not be loaded")
    })?;
    let artifacts_by_event_id = artifacts
        .into_iter()
        .map(|artifact| (artifact.event_id, artifact.message_json))
        .collect::<HashMap<_, _>>();
    if artifacts_by_event_id.len() != parent_event_ids.len() {
        return Err(replay_unavailable(
            "stored replay state is missing an assistant artifact for a tool-call turn",
        ));
    }

    restore_reasoning_from_replay(messages, &parent_by_call_id, &artifacts_by_event_id)?;
    serde_json::to_vec(&value)
        .map(Some)
        .map_err(|_| replay_unavailable("failed to encode the restored chat request"))
}

fn restore_reasoning_from_replay(
    messages: &mut [Value],
    parent_by_call_id: &HashMap<String, i64>,
    artifacts_by_event_id: &HashMap<i64, Value>,
) -> Result<(), crate::openai_compat::CompatError> {
    for (assistant_index, call_ids) in assistant_tool_call_refs(messages)? {
        let parent_event_id = call_ids
            .iter()
            .map(|call_id| parent_by_call_id.get(call_id).copied())
            .collect::<Option<Vec<_>>>()
            .and_then(|parent_ids| {
                let unique_parent_ids = parent_ids.into_iter().collect::<HashSet<_>>();
                (unique_parent_ids.len() == 1)
                    .then(|| unique_parent_ids.into_iter().next().expect("one parent"))
            })
            .ok_or_else(|| {
                replay_unavailable(
                    "assistant tool-call message mixes replay parents or has an unknown call id",
                )
            })?;
        let artifact = artifacts_by_event_id.get(&parent_event_id).ok_or_else(|| {
            replay_unavailable("stored replay state is missing an assistant tool-call artifact")
        })?;
        let artifact_message = persisted_assistant_message(artifact)
            .map_err(|_| replay_unavailable("stored assistant tool-call artifact is invalid"))?;
        if !tool_calls_match(&messages[assistant_index], &artifact_message) {
            return Err(replay_unavailable(
                "stored assistant tool-call artifact does not match the replayed request",
            ));
        }
        let reasoning_content = artifact_message
            .get("reasoning_content")
            .cloned()
            .filter(has_meaningful_value)
            .ok_or_else(|| {
                replay_unavailable(
                    "stored assistant tool-call turn is missing complete reasoning for the target reasoning provider",
                )
            })?;
        let assistant = messages[assistant_index].as_object_mut().ok_or_else(|| {
            replay_unavailable("replayed assistant tool-call message is not a JSON object")
        })?;
        if let Some(tool_calls) = artifact_message.get("tool_calls").cloned() {
            assistant.insert("tool_calls".to_string(), tool_calls);
        }
        assistant.insert("reasoning_content".to_string(), reasoning_content);
    }
    Ok(())
}

fn assistant_tool_call_ids(
    messages: &[Value],
) -> Result<Vec<String>, crate::openai_compat::CompatError> {
    Ok(assistant_tool_call_refs(messages)?
        .into_iter()
        .flat_map(|(_, call_ids)| call_ids)
        .collect())
}

fn assistant_tool_call_refs(
    messages: &[Value],
) -> Result<Vec<(usize, Vec<String>)>, crate::openai_compat::CompatError> {
    let mut refs = Vec::new();
    let mut seen_call_ids = HashSet::new();
    for (index, message) in messages.iter().enumerate() {
        let Some(object) = message.as_object() else {
            continue;
        };
        if object.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(tool_calls_value) = object.get("tool_calls") else {
            continue;
        };
        if tool_calls_value.is_null() {
            continue;
        }
        let tool_calls = tool_calls_value.as_array().ok_or_else(|| {
            replay_unavailable("assistant tool_calls must be an array for reasoning recovery")
        })?;
        let mut call_ids = Vec::with_capacity(tool_calls.len());
        for tool_call in tool_calls {
            let call_id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|call_id| !call_id.is_empty())
                .ok_or_else(|| {
                    replay_unavailable(
                        "assistant tool-call history contains a tool call without an id",
                    )
                })?
                .to_string();
            if !seen_call_ids.insert(call_id.clone()) {
                return Err(replay_unavailable(
                    "assistant tool-call history contains a duplicate call id",
                ));
            }
            call_ids.push(call_id);
        }
        if !call_ids.is_empty() {
            refs.push((index, call_ids));
        }
    }
    Ok(refs)
}

fn tool_calls_match(current: &Value, artifact: &Value) -> bool {
    tool_call_signatures(current) == tool_call_signatures(artifact)
}

fn tool_call_signatures(value: &Value) -> Option<HashMap<String, (String, String)>> {
    let tool_calls = value.get("tool_calls").and_then(Value::as_array)?;
    let mut signatures = HashMap::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let object = tool_call.as_object()?;
        let id = object.get("id").and_then(Value::as_str)?.to_string();
        let function = object.get("function").and_then(Value::as_object)?;
        let name = function.get("name").and_then(Value::as_str)?.to_string();
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if signatures.insert(id, (name, arguments)).is_some() {
            return None;
        }
    }
    Some(signatures)
}

fn replay_unavailable(message: impl Into<String>) -> crate::openai_compat::CompatError {
    crate::openai_compat::CompatError::new(StatusCode::BAD_REQUEST, "replay_unavailable", message)
}

fn targets_reasoning_provider(route: &db::RouteConfig, model: Option<&str>) -> bool {
    targets_deepseek(route, model) || targets_minimax(route, model)
}

fn targets_deepseek(route: &db::RouteConfig, model: Option<&str>) -> bool {
    route.base_url.to_ascii_lowercase().contains("deepseek")
        || model.is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("deepseek-"))
}

fn targets_minimax(route: &db::RouteConfig, model: Option<&str>) -> bool {
    route.base_url.to_ascii_lowercase().contains("minimax")
        || model.is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("minimax-"))
}

fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}

#[cfg(test)]
#[path = "provider_reasoning_tests.rs"]
mod tests;
