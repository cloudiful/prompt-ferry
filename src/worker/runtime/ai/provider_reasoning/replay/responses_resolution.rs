use super::{ReplayFailureKind, replay_unavailable, signature_hash, tool_calls_match};
use crate::openai_compat::persisted_assistant_message;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use tracing::warn;

#[derive(Debug, Clone)]
pub(crate) struct ResponsesReplayToolCall {
    pub(crate) input_index: usize,
    pub(crate) call_id: String,
    pub(crate) tool_call: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResponsesReplayGroup {
    pub(crate) first_index: usize,
    pub(crate) parent_event_id: i64,
}

struct GroupState {
    first_index: usize,
    last_index: usize,
    parent_event_id: i64,
    tool_calls: Vec<Value>,
}

pub(crate) fn resolve_responses_replay_groups(
    calls: &[ResponsesReplayToolCall],
    candidates_by_call_id: &HashMap<String, Vec<(i64, bool)>>,
    artifacts_by_event_id: &HashMap<i64, Value>,
    has_provenance: bool,
) -> Result<Vec<ResponsesReplayGroup>, crate::openai_compat::CompatError> {
    let mut assignments = Vec::with_capacity(calls.len());
    for call in calls {
        let candidates = candidates_by_call_id
            .get(&call.call_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let candidate_parent_ids = candidates
            .iter()
            .map(|(parent_id, _)| *parent_id)
            .collect::<HashSet<_>>();
        let live_parent_ids = candidates
            .iter()
            .filter_map(|(parent_id, has_artifact)| has_artifact.then_some(*parent_id))
            .collect::<HashSet<_>>();

        if live_parent_ids.is_empty() {
            warn!(
                failure_kind = ReplayFailureKind::MissingArtifact.as_str(),
                input_index = call.input_index,
                call_id = call.call_id,
                candidate_parent_ids = ?candidate_parent_ids,
                "provider Responses replay call has no live assistant artifact"
            );
            return Err(replay_unavailable(
                ReplayFailureKind::MissingArtifact,
                format!(
                    "stored replay state is missing an assistant artifact for tool call `{}`",
                    call.call_id
                ),
            ));
        }

        if !has_provenance && candidate_parent_ids.len() > 1 {
            warn!(
                failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                input_index = call.input_index,
                call_id = call.call_id,
                candidate_parent_ids = ?candidate_parent_ids,
                live_parent_ids = ?live_parent_ids,
                "provider Responses replay call has multiple parents without provenance"
            );
            return Err(replay_unavailable(
                ReplayFailureKind::AmbiguousParent,
                format!(
                    "tool call `{}` does not resolve to a unique replay parent without conversation provenance",
                    call.call_id
                ),
            ));
        }

        let mut matching_parent_ids = Vec::new();
        for parent_event_id in live_parent_ids {
            let artifact = artifacts_by_event_id.get(&parent_event_id).ok_or_else(|| {
                replay_unavailable(
                    ReplayFailureKind::MissingArtifact,
                    "stored replay state is missing an assistant artifact for a candidate tool call",
                )
            })?;
            if artifact_contains_tool_call(artifact, &call.tool_call)? {
                matching_parent_ids.push(parent_event_id);
            }
        }

        match matching_parent_ids.as_slice() {
            [parent_event_id] => assignments.push((call, *parent_event_id)),
            [] => {
                warn!(
                    failure_kind = ReplayFailureKind::SignatureMismatch.as_str(),
                    input_index = call.input_index,
                    call_id = call.call_id,
                    candidate_parent_ids = ?candidate_parent_ids,
                    "provider Responses replay tool call does not match any candidate artifact"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::SignatureMismatch,
                    "stored assistant tool-call artifact does not match the replayed Responses request",
                ));
            }
            _ => {
                warn!(
                    failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                    input_index = call.input_index,
                    call_id = call.call_id,
                    candidate_parent_ids = ?matching_parent_ids,
                    "provider Responses replay tool call matches multiple assistant artifacts"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::AmbiguousParent,
                    "replayed Responses tool call matches multiple assistant artifacts",
                ));
            }
        }
    }

    let mut groups = Vec::<GroupState>::new();
    for (call, parent_event_id) in assignments {
        let append = groups
            .last()
            .is_some_and(|group| group.parent_event_id == parent_event_id);
        if append {
            let group = groups.last_mut().expect("group exists after append check");
            group.last_index = call.input_index;
            group.tool_calls.push(call.tool_call.clone());
        } else {
            groups.push(GroupState {
                first_index: call.input_index,
                last_index: call.input_index,
                parent_event_id,
                tool_calls: vec![call.tool_call.clone()],
            });
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let artifact = artifacts_by_event_id.get(&group.parent_event_id).ok_or_else(|| {
                replay_unavailable(
                    ReplayFailureKind::MissingArtifact,
                    "stored replay state is missing an assistant artifact for a Responses tool-call turn",
                )
            })?;
            let artifact_message = persisted_assistant_message(artifact).map_err(|_| {
                replay_unavailable(
                    ReplayFailureKind::InvalidHistory,
                    "stored Responses assistant tool-call artifact is invalid",
                )
            })?;
            let message = json!({
                "role": "assistant",
                "content": null,
                "tool_calls": group.tool_calls,
            });
            if !tool_calls_match(&message, &artifact_message) {
                warn!(
                    failure_kind = ReplayFailureKind::SignatureMismatch.as_str(),
                    parent_event_id = group.parent_event_id,
                    group_call_ids = ?group_call_ids(&group.tool_calls),
                    artifact_call_ids = ?artifact_call_ids(artifact),
                    group_signature_hash = ?signature_hash(&message),
                    artifact_signature_hash = ?signature_hash(&artifact_message),
                    first_input_index = group.first_index,
                    last_input_index = group.last_index,
                    artifact_source = artifact_source_label(artifact),
                    "provider Responses replay group tool-call signature does not match stored artifact"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::SignatureMismatch,
                    "stored assistant tool-call artifact does not match one resolved Responses turn",
                ));
            }
            Ok(ResponsesReplayGroup {
                first_index: group.first_index,
                parent_event_id: group.parent_event_id,
            })
        })
        .collect()
}

fn artifact_call_ids(artifact: &Value) -> Vec<String> {
    artifact
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|tool_call| tool_call.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn group_call_ids(tool_calls: &[Value]) -> Vec<String> {
    tool_calls
        .iter()
        .filter_map(|tool_call| tool_call.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn artifact_source_label(artifact: &Value) -> &'static str {
    if artifact
        .get("assistant_message")
        .and_then(Value::as_object)
        .is_some()
    {
        "assistant_message"
    } else {
        "output_items"
    }
}

fn artifact_contains_tool_call(
    artifact: &Value,
    current_tool_call: &Value,
) -> Result<bool, crate::openai_compat::CompatError> {
    let artifact_message = persisted_assistant_message(artifact).map_err(|_| {
        replay_unavailable(
            ReplayFailureKind::InvalidHistory,
            "stored Responses assistant tool-call artifact is invalid",
        )
    })?;
    let Some(artifact_tool_calls) = artifact_message.get("tool_calls").and_then(Value::as_array)
    else {
        return Ok(false);
    };
    let current_message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [current_tool_call],
    });
    Ok(artifact_tool_calls.iter().any(|artifact_tool_call| {
        tool_calls_match(
            &current_message,
            &json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [artifact_tool_call],
            }),
        )
    }))
}

#[cfg(test)]
#[path = "responses_resolution_tests.rs"]
mod tests;
