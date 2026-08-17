use crate::{db, openai_compat::persisted_assistant_message};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tracing::warn;

mod resolution;
mod signatures;
mod storage;

pub(super) use resolution::resolve_replay_parents;
use signatures::signature_hash;
pub(super) use signatures::tool_calls_match;
pub(super) use storage::load_tool_call_replay_state;

#[derive(Debug, Copy, Clone)]
pub(super) enum ReplayFailureKind {
    MissingArtifact,
    AmbiguousParent,
    SignatureMismatch,
    MissingReasoning,
    InvalidHistory,
    Storage,
}

impl ReplayFailureKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::MissingArtifact => "missing_artifact",
            Self::AmbiguousParent => "ambiguous_parent",
            Self::SignatureMismatch => "signature_mismatch",
            Self::MissingReasoning => "missing_reasoning",
            Self::InvalidHistory => "invalid_history",
            Self::Storage => "storage_error",
        }
    }
}

pub(super) fn restore_reasoning_from_replay(
    messages: &mut [Value],
    parent_by_assistant_index: &HashMap<usize, i64>,
    artifacts_by_event_id: &HashMap<i64, Value>,
) -> Result<(), crate::openai_compat::CompatError> {
    for (assistant_index, call_ids) in assistant_tool_call_refs(messages)? {
        let Some(parent_event_id) = parent_by_assistant_index.get(&assistant_index).copied() else {
            warn!(
                failure_kind = ReplayFailureKind::AmbiguousParent.as_str(),
                call_ids = ?call_ids,
                assistant_index,
                "provider reasoning replay has no resolved assistant tool-call parent"
            );
            return Err(replay_unavailable(
                ReplayFailureKind::AmbiguousParent,
                "assistant tool-call history has no safely resolved replay parent",
            ));
        };
        let artifact = artifacts_by_event_id.get(&parent_event_id).ok_or_else(|| {
            warn!(
                failure_kind = ReplayFailureKind::MissingArtifact.as_str(),
                call_ids = ?call_ids,
                parent_event_id,
                "provider reasoning replay artifact is missing"
            );
            replay_unavailable(
                ReplayFailureKind::MissingArtifact,
                format!(
                    "stored replay state is missing an assistant tool-call artifact for event `{parent_event_id}`"
                ),
            )
        })?;
        let artifact_message = persisted_assistant_message(artifact).map_err(|_| {
            replay_unavailable(
                ReplayFailureKind::InvalidHistory,
                "stored assistant tool-call artifact is invalid",
            )
        })?;
        if !tool_calls_match(&messages[assistant_index], &artifact_message) {
            warn!(
                failure_kind = ReplayFailureKind::SignatureMismatch.as_str(),
                call_ids = ?call_ids,
                parent_event_id,
                request_signature_hash = ?signature_hash(&messages[assistant_index]),
                artifact_signature_hash = ?signature_hash(&artifact_message),
                "provider reasoning replay tool-call signature does not match"
            );
            return Err(replay_unavailable(
                ReplayFailureKind::SignatureMismatch,
                "stored assistant tool-call artifact does not match the replayed request",
            ));
        }
        let reasoning_content = artifact_message
            .get("reasoning_content")
            .cloned()
            .filter(has_meaningful_value)
            .ok_or_else(|| {
                warn!(
                    failure_kind = ReplayFailureKind::MissingReasoning.as_str(),
                    call_ids = ?call_ids,
                    parent_event_id,
                    "provider reasoning replay artifact has no reasoning content"
                );
                replay_unavailable(
                    ReplayFailureKind::MissingReasoning,
                    format!(
                        "stored assistant tool-call turn (event `{parent_event_id}`) is missing complete reasoning for the target reasoning provider"
                    ),
                )
            })?;
        let assistant = messages[assistant_index].as_object_mut().ok_or_else(|| {
            replay_unavailable(
                ReplayFailureKind::InvalidHistory,
                "replayed assistant tool-call message is not a JSON object",
            )
        })?;
        if let Some(tool_calls) = artifact_message.get("tool_calls").cloned() {
            assistant.insert("tool_calls".to_string(), tool_calls);
        }
        assistant.insert("reasoning_content".to_string(), reasoning_content);
    }
    Ok(())
}

pub(super) fn assistant_tool_call_ids(
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
            replay_unavailable(
                ReplayFailureKind::InvalidHistory,
                "assistant tool_calls must be an array for reasoning recovery",
            )
        })?;
        let mut call_ids = Vec::with_capacity(tool_calls.len());
        let mut turn_call_ids = HashSet::with_capacity(tool_calls.len());
        for tool_call in tool_calls {
            let call_id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|call_id| !call_id.is_empty())
                .ok_or_else(|| {
                    replay_unavailable(
                        ReplayFailureKind::InvalidHistory,
                        "assistant tool-call history contains a tool call without an id",
                    )
                })?
                .to_string();
            if !turn_call_ids.insert(call_id.clone()) {
                return Err(replay_unavailable(
                    ReplayFailureKind::InvalidHistory,
                    "assistant tool-call history contains a duplicate call id in one turn",
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

pub(super) fn replay_unavailable(
    kind: ReplayFailureKind,
    message: impl Into<String>,
) -> crate::openai_compat::CompatError {
    let message = message.into();
    warn!(
        failure_kind = kind.as_str(),
        error_code = "replay_unavailable",
        stage = "before_forward",
        error_message = %message,
        "provider reasoning replay is unavailable before forwarding"
    );
    crate::openai_compat::CompatError::new(
        StatusCode::BAD_REQUEST,
        "replay_unavailable",
        format!(
            "{}: {message} The request was rejected before forwarding; the upstream provider was never called. Start a new conversation or remove the old tool-call history.",
            kind.as_str()
        ),
    )
}

/// Whether chat request history should have reasoning restored for the route.
///
/// - `force_passthrough` never restores: history is forwarded untouched and the
///   upstream decides whether it accepts the request.
/// - `force_replay` always restores, overriding endpoint/model judgment for
///   proxies that are known to forward to a reasoning provider.
/// - `auto` requires both a recognized direct provider base URL and a known
///   reasoning model, so aggregate proxies are no longer classified as direct
///   reasoning providers by model name prefix alone.
pub(super) fn chat_reasoning_replay_enabled(route: &db::RouteConfig, model: Option<&str>) -> bool {
    match route.chat_reasoning_replay_policy {
        db::ChatReasoningReplayPolicy::ForcePassthrough => false,
        db::ChatReasoningReplayPolicy::ForceReplay => true,
        db::ChatReasoningReplayPolicy::Auto => {
            (direct_deepseek_endpoint(route) && deepseek_reasoning_model(model))
                || (direct_minimax_endpoint(route) && minimax_reasoning_model(model))
        }
    }
}

/// Whether Responses input items should have reasoning restored for the route.
pub(super) fn responses_reasoning_replay_enabled(
    route: &db::RouteConfig,
    model: Option<&str>,
) -> bool {
    match route.chat_reasoning_replay_policy {
        db::ChatReasoningReplayPolicy::ForcePassthrough => false,
        db::ChatReasoningReplayPolicy::ForceReplay => true,
        db::ChatReasoningReplayPolicy::Auto => {
            (direct_deepseek_endpoint(route) && deepseek_reasoning_model(model))
                || (direct_minimax_endpoint(route) && minimax_reasoning_model(model))
        }
    }
}

/// Why the route was recognized as a reasoning replay target (or not).
pub(super) fn replay_recognition_basis(
    route: &db::RouteConfig,
    model: Option<&str>,
) -> &'static str {
    match route.chat_reasoning_replay_policy {
        db::ChatReasoningReplayPolicy::ForcePassthrough => "policy_force_passthrough",
        db::ChatReasoningReplayPolicy::ForceReplay => "policy_force_replay",
        db::ChatReasoningReplayPolicy::Auto if chat_reasoning_replay_enabled(route, model) => {
            "direct_endpoint_and_reasoning_model"
        }
        db::ChatReasoningReplayPolicy::Auto => {
            if direct_deepseek_endpoint(route) || direct_minimax_endpoint(route) {
                "direct_endpoint_without_reasoning_model"
            } else {
                "no_direct_reasoning_endpoint"
            }
        }
    }
}

pub(super) fn direct_deepseek_endpoint(route: &db::RouteConfig) -> bool {
    route.base_url.to_ascii_lowercase().contains("deepseek")
}

pub(super) fn direct_minimax_endpoint(route: &db::RouteConfig) -> bool {
    route.base_url.to_ascii_lowercase().contains("minimax")
}

pub(super) fn deepseek_reasoning_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| {
        let model = model.trim().to_ascii_lowercase();
        model.starts_with("deepseek-reasoner") || model.starts_with("deepseek-v4")
    })
}

pub(super) fn minimax_reasoning_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| {
        let model = model.trim().to_ascii_lowercase();
        model.starts_with("minimax-m1")
            || model.starts_with("minimax-m2")
            || model.starts_with("minimax-m3")
    })
}

/// Whether the route targets a Minimax reasoning provider for chat body
/// defaults (`reasoning_split`, `reasoning_details` conversion).
///
/// `force_passthrough` never rewrites chat history; `force_replay` keeps the
/// legacy endpoint-or-model-prefix judgment for proxies that forward to
/// Minimax; `auto` requires a direct Minimax base URL and a known reasoning
/// model.
pub(crate) fn targets_minimax(route: &db::RouteConfig, model: Option<&str>) -> bool {
    match route.chat_reasoning_replay_policy {
        db::ChatReasoningReplayPolicy::ForcePassthrough => false,
        db::ChatReasoningReplayPolicy::ForceReplay => {
            route.base_url.to_ascii_lowercase().contains("minimax")
                || model
                    .is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("minimax-"))
        }
        db::ChatReasoningReplayPolicy::Auto => {
            direct_minimax_endpoint(route) && minimax_reasoning_model(model)
        }
    }
}

pub(super) fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}
