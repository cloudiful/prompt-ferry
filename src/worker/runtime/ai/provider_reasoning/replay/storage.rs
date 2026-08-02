use super::{ReplayFailureKind, replay_unavailable};
use crate::{db, worker_admin::AdminState};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

pub(crate) async fn load_tool_call_replay_state(
    state: &AdminState,
    call_ids: &[String],
    user_id: Option<i64>,
    endpoint_id: Option<uuid::Uuid>,
    conversation_id: Option<uuid::Uuid>,
    parent_event_id: Option<i64>,
) -> Result<
    (HashMap<String, Vec<(i64, bool)>>, HashMap<i64, Value>),
    crate::openai_compat::CompatError,
> {
    let records = db::find_request_record_tool_calls_by_call_ids(
        &state.pool,
        call_ids,
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
        .filter_map(|(event_id, has_artifact)| has_artifact.then_some(*event_id))
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
        .collect();

    Ok((candidates_by_call_id, artifacts_by_event_id))
}
