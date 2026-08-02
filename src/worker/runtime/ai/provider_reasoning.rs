use crate::{db, upstream_adapter::ResponseAdapter, worker_admin::AdminState};
use serde_json::Value;

mod replay;
mod responses;

use replay::{
    ReplayFailureKind, assistant_tool_call_ids, load_tool_call_replay_state, replay_unavailable,
    resolve_replay_parents, restore_reasoning_from_replay, targets_deepseek,
    targets_reasoning_provider,
};

pub(super) use responses::restore_responses_reasoning;

#[cfg(test)]
use replay::tool_calls_match;

pub(super) async fn restore_provider_reasoning(
    admin_state: Option<&AdminState>,
    user_id: Option<i64>,
    route: &db::RouteConfig,
    conversation_id: Option<uuid::Uuid>,
    parent_event_id: Option<i64>,
    response_adapter: ResponseAdapter,
    request_body: &[u8],
) -> Result<Option<Vec<u8>>, crate::openai_compat::CompatError> {
    let Ok(mut value) = serde_json::from_slice::<Value>(request_body) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    let model = object.get("model").and_then(Value::as_str);
    if response_adapter == ResponseAdapter::ChatToResponses && targets_deepseek(route, model) {
        return Ok(None);
    }
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
            ReplayFailureKind::MissingArtifact,
            "cannot restore reasoning for assistant tool calls without stored replay state",
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
    let has_provenance = conversation_id.is_some() || parent_event_id.is_some();
    let parent_by_assistant_index = resolve_replay_parents(
        messages,
        &candidates_by_call_id,
        &artifacts_by_event_id,
        has_provenance,
    )?;

    restore_reasoning_from_replay(messages, &parent_by_assistant_index, &artifacts_by_event_id)?;
    serde_json::to_vec(&value).map(Some).map_err(|_| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "failed to encode the restored chat request",
        )
    })
}

#[cfg(test)]
#[path = "provider_reasoning_tests.rs"]
mod tests;
