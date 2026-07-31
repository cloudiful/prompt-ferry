use crate::{db, worker_admin::AdminState};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

mod replay;

use replay::{
    ReplayFailureKind, assistant_tool_call_ids, replay_unavailable, resolve_replay_parents,
    restore_reasoning_from_replay, targets_reasoning_provider,
};

#[cfg(test)]
use replay::tool_calls_match;

pub(super) async fn restore_provider_reasoning(
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
            ReplayFailureKind::MissingArtifact,
            "cannot restore reasoning for assistant tool calls without stored replay state",
        )
    })?;
    let endpoint_id = Some(route.route_id).filter(|id| !id.is_nil());
    let records = db::find_request_record_tool_calls_by_call_ids(
        &state.pool,
        &requested_call_ids,
        user_id,
        endpoint_id,
        conversation_id,
        parent_event_id,
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "failed to load provider tool-call replay records");
        replay_unavailable(
            ReplayFailureKind::Storage,
            "stored tool-call replay records could not be loaded",
        )
    })?;

    let has_provenance = conversation_id.is_some() || parent_event_id.is_some();
    let mut candidates_by_call_id: HashMap<String, Vec<(i64, bool)>> = HashMap::new();
    for candidate in records {
        let tool_call = candidate.tool_call;
        candidates_by_call_id
            .entry(tool_call.call_id)
            .or_default()
            .push((tool_call.parent_event_id, candidate.has_assistant_artifact));
    }
    let candidate_parent_event_ids = candidates_by_call_id
        .values()
        .flatten()
        .filter_map(|(parent_event_id, has_artifact)| has_artifact.then_some(*parent_event_id))
        .collect::<HashSet<_>>();
    let artifacts = db::get_usage_assistant_artifacts(
        &state.pool,
        &candidate_parent_event_ids
            .iter()
            .copied()
            .collect::<Vec<_>>(),
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "failed to load provider assistant replay artifacts");
        replay_unavailable(
            ReplayFailureKind::Storage,
            "stored assistant replay artifacts could not be loaded",
        )
    })?;
    let artifacts_by_event_id = artifacts
        .into_iter()
        .map(|artifact| (artifact.event_id, artifact.message_json))
        .collect::<HashMap<_, _>>();
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
