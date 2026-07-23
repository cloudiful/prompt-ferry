use crate::{db, openai_compat::persisted_assistant_message, worker_admin::AdminState};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

pub(super) async fn restore_provider_reasoning(
    admin_state: Option<&AdminState>,
    user_id: Option<i64>,
    route: &db::RouteConfig,
    request_body: &[u8],
) -> Option<Vec<u8>> {
    let mut value = serde_json::from_slice::<Value>(request_body).ok()?;
    let object = value.as_object_mut()?;
    let model = object.get("model").and_then(Value::as_str);
    if !targets_reasoning_provider(route, model) {
        return None;
    }
    let messages = object.get_mut("messages").and_then(Value::as_array_mut)?;
    let (assistant_index, call_ids) = latest_assistant_tool_call(messages)?;
    let requested_call_ids = call_ids.iter().cloned().collect::<HashSet<_>>();
    if requested_call_ids.len() != call_ids.len() {
        return None;
    }
    let state = admin_state?;
    let endpoint_id = Some(route.route_id).filter(|id| !id.is_nil());
    let records = match db::find_request_record_tool_calls_by_call_ids(
        &state.pool,
        &call_ids,
        user_id,
        endpoint_id,
    )
    .await
    {
        Ok(records) => records,
        Err(err) => {
            warn!(error = %err, "failed to load provider tool-call replay records");
            return None;
        }
    };
    let record_call_ids = records
        .iter()
        .map(|record| record.call_id.clone())
        .collect::<HashSet<_>>();
    let parent_event_ids = records
        .iter()
        .map(|record| record.parent_event_id)
        .collect::<HashSet<_>>();
    if records.len() != call_ids.len()
        || record_call_ids != requested_call_ids
        || parent_event_ids.len() != 1
    {
        return None;
    }
    let parent_event_id = *parent_event_ids.iter().next()?;
    let artifacts = match db::get_usage_assistant_artifacts(&state.pool, &[parent_event_id]).await {
        Ok(artifacts) => artifacts,
        Err(err) => {
            warn!(
                error = %err,
                parent_event_id,
                "failed to load provider assistant replay artifact"
            );
            return None;
        }
    };
    let artifact = artifacts
        .into_iter()
        .find(|artifact| artifact.event_id == parent_event_id)?;
    let artifact_message = persisted_assistant_message(&artifact.message_json).ok()?;
    if !tool_calls_match(&messages[assistant_index], &artifact_message) {
        return None;
    }
    let reasoning_content = artifact_message
        .get("reasoning_content")
        .cloned()
        .filter(has_meaningful_value)?;
    let assistant = messages[assistant_index].as_object_mut()?;
    if let Some(tool_calls) = artifact_message.get("tool_calls").cloned() {
        assistant.insert("tool_calls".to_string(), tool_calls);
    }
    assistant.insert("reasoning_content".to_string(), reasoning_content);
    serde_json::to_vec(&value).ok()
}

fn latest_assistant_tool_call(messages: &[Value]) -> Option<(usize, Vec<String>)> {
    messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| {
            let object = message.as_object()?;
            if object.get("role").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            let tool_calls = object.get("tool_calls").and_then(Value::as_array)?;
            let call_ids = tool_calls
                .iter()
                .filter_map(|tool_call| tool_call.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            (!call_ids.is_empty()).then_some((index, call_ids))
        })
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
