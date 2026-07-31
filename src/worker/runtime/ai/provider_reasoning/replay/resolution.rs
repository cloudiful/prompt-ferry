use super::{
    ReplayFailureKind, assistant_tool_call_refs, replay_unavailable, signature_hash,
    tool_calls_match,
};
use crate::openai_compat::persisted_assistant_message;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

pub fn resolve_replay_parents(
    messages: &[Value],
    candidates_by_call_id: &HashMap<String, Vec<(i64, bool)>>,
    artifacts_by_event_id: &HashMap<i64, Value>,
    has_provenance: bool,
) -> Result<HashMap<usize, i64>, crate::openai_compat::CompatError> {
    let mut parent_by_assistant_index = HashMap::new();
    for (assistant_index, call_ids) in assistant_tool_call_refs(messages)? {
        let mut live_parent_ids: Option<HashSet<i64>> = None;
        let mut all_parent_ids = HashSet::new();

        for call_id in &call_ids {
            let candidates = candidates_by_call_id
                .get(call_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let candidate_parent_ids = candidates
                .iter()
                .map(|(parent_id, _)| *parent_id)
                .collect::<HashSet<_>>();
            all_parent_ids.extend(candidate_parent_ids.iter().copied());
            let call_live_parent_ids = candidates
                .iter()
                .filter_map(|(parent_id, has_artifact)| has_artifact.then_some(*parent_id))
                .collect::<HashSet<_>>();

            if call_live_parent_ids.is_empty() {
                warn!(
                    failure_kind = ReplayFailureKind::MissingArtifact.as_str(),
                    assistant_index,
                    call_id,
                    candidate_parent_ids = ?candidate_parent_ids,
                    "provider reasoning replay call has no live assistant artifact"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::MissingArtifact,
                    format!(
                        "stored replay state is missing an assistant artifact for tool call `{call_id}`"
                    ),
                ));
            }

            if !has_provenance && candidate_parent_ids.len() > 1 {
                warn!(
                    failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                    assistant_index,
                    call_id,
                    candidate_parent_ids = ?candidate_parent_ids,
                    live_parent_ids = ?call_live_parent_ids,
                    "provider reasoning replay call has multiple parents without provenance"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::AmbiguousParent,
                    format!(
                        "tool call `{call_id}` does not resolve to a unique replay parent without conversation provenance"
                    ),
                ));
            }

            live_parent_ids = Some(match live_parent_ids {
                Some(current) => current
                    .intersection(&call_live_parent_ids)
                    .copied()
                    .collect(),
                None => call_live_parent_ids,
            });
        }

        let live_parent_ids = live_parent_ids.unwrap_or_default();
        if live_parent_ids.is_empty() {
            warn!(
                failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                assistant_index,
                call_ids = ?call_ids,
                candidate_parent_ids = ?all_parent_ids,
                "provider reasoning replay assistant turn mixes replay parents"
            );
            return Err(replay_unavailable(
                ReplayFailureKind::AmbiguousParent,
                "assistant tool-call history mixes replay parents that cannot form one assistant turn",
            ));
        }

        let mut matching_parent_ids = Vec::new();
        for parent_event_id in &live_parent_ids {
            let Some(artifact) = artifacts_by_event_id.get(parent_event_id) else {
                warn!(
                    failure_kind = ReplayFailureKind::MissingArtifact.as_str(),
                    assistant_index,
                    call_ids = ?call_ids,
                    parent_event_id,
                    "provider reasoning replay candidate artifact disappeared before validation"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::MissingArtifact,
                    "stored replay state is missing an assistant artifact for a candidate tool-call turn",
                ));
            };
            let artifact_message = match persisted_assistant_message(artifact) {
                Ok(message) => message,
                Err(_) => {
                    return Err(replay_unavailable(
                        ReplayFailureKind::InvalidHistory,
                        "stored assistant tool-call artifact is invalid",
                    ));
                }
            };
            if tool_calls_match(&messages[assistant_index], &artifact_message) {
                matching_parent_ids.push(*parent_event_id);
            }
        }

        match matching_parent_ids.as_slice() {
            [parent_event_id] => {
                parent_by_assistant_index.insert(assistant_index, *parent_event_id);
            }
            [] => {
                warn!(
                    failure_kind = ReplayFailureKind::SignatureMismatch.as_str(),
                    assistant_index,
                    call_ids = ?call_ids,
                    candidate_parent_ids = ?live_parent_ids,
                    request_signature_hash = ?signature_hash(&messages[assistant_index]),
                    artifact_signature_hashes = ?live_parent_ids.iter().filter_map(|parent_id| {
                        artifacts_by_event_id
                            .get(parent_id)
                            .and_then(|artifact| persisted_assistant_message(artifact).ok())
                            .and_then(|message| signature_hash(&message))
                    }).collect::<Vec<_>>(),
                    "provider reasoning replay tool-call signature does not match any candidate parent"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::SignatureMismatch,
                    "stored assistant tool-call artifact does not match the replayed request",
                ));
            }
            _ => {
                warn!(
                    failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                    assistant_index,
                    call_ids = ?call_ids,
                    candidate_parent_ids = ?matching_parent_ids,
                    "provider reasoning replay signature matches multiple assistant parents"
                );
                return Err(replay_unavailable(
                    ReplayFailureKind::AmbiguousParent,
                    "assistant tool-call history matches multiple replay parents",
                ));
            }
        }
    }
    Ok(parent_by_assistant_index)
}
