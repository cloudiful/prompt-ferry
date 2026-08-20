use std::collections::HashSet;

use http::StatusCode;
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    anthropic_compat::responses_request_to_anthropic_messages,
    config::NativeApi,
    db,
    openai_compat::{
        CompatError, NormalizedResponsesRequest, conversation_key, output_items_to_input_items,
        persisted_output_items, previous_response_id,
    },
    replay_cache::ReplayCache,
};

mod history;
mod reconstruct;

#[cfg(test)]
pub(super) use history::fallback_artifact_for_entry;
#[cfg(test)]
pub(super) use reconstruct::{replay_assistant_message, replayable_output_items};

pub struct ResponsesReplayRequest<'a> {
    pub pool: &'a PgPool,
    pub replay_cache: &'a ReplayCache,
    pub user_id: Option<i64>,
    pub resolved_parent_event_id: Option<i64>,
    pub request_body: &'a [u8],
    pub native_api: NativeApi,
    pub route_base_url: &'a str,
    pub current_request_model: Option<&'a str>,
}

pub async fn prepare_responses_replay_request(
    input: ResponsesReplayRequest<'_>,
) -> Result<Vec<u8>, CompatError> {
    let ResponsesReplayRequest {
        pool,
        replay_cache,
        user_id,
        resolved_parent_event_id,
        request_body,
        native_api,
        route_base_url: _route_base_url,
        current_request_model: _current_request_model,
    } = input;
    let request = NormalizedResponsesRequest::from_body(request_body)?;
    let parent = if let Some(parent_event_id) = resolved_parent_event_id {
        db::get_usage_event_chain_entry(pool, parent_event_id)
            .await
            .map_err(replay_db_error)?
            .ok_or_else(|| {
                replay_error(format!(
                    "cannot replay continuation: inferred parent event `{parent_event_id}` was not found in stored history"
                ))
            })?
    } else if let Some(previous_response_id) = previous_response_id(request_body) {
        db::get_usage_event_by_provider_response_id(pool, user_id, &previous_response_id)
            .await
            .map_err(replay_db_error)?
            .ok_or_else(|| {
                replay_error(format!(
                    "cannot replay chat-native continuation: previous_response_id `{previous_response_id}` was not found in stored history"
                ))
            })?
    } else if let Some(provider_conversation_key) = request.conversation.as_deref() {
        db::get_replayable_usage_event_by_provider_conversation_key(
            pool,
            user_id,
            provider_conversation_key,
        )
        .await
        .map_err(replay_db_error)?
        .ok_or_else(|| {
            replay_error(format!(
                "cannot replay continuation: conversation `{provider_conversation_key}` was not found in stored history"
            ))
        })?
    } else {
        return Err(replay_error(
            "previous_response_id or conversation is required for replay assembly",
        ));
    };

    let history_entries = history::load_history_entries(pool, replay_cache, &parent).await?;
    let assistant_artifacts = history::load_assistant_artifacts(pool, &history_entries).await?;
    let prior_call_ids = assistant_artifacts
        .values()
        .flat_map(|artifact| {
            persisted_output_items(&artifact.message_json)
                .unwrap_or_default()
                .into_iter()
        })
        .collect::<Vec<_>>();
    request.validate_for_chat_compat(
        &prior_call_ids
            .iter()
            .filter_map(|item| item.get("call_id").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<HashSet<_>>(),
        true,
    )?;

    match native_api {
        NativeApi::Chat => {
            let mut prefix_messages = Vec::new();
            for entry in &history_entries {
                prefix_messages.extend(reconstruct::reconstruct_turn_messages(pool, entry).await?);
                let artifact = assistant_artifacts.get(&entry.event_id).ok_or_else(|| {
                    replay_error("missing assistant replay artifact for prior turn")
                })?;
                prefix_messages.push(reconstruct::replay_assistant_message(
                    &artifact.message_json,
                    false,
                )?);
            }
            request.to_chat_request_with_prefix(&prefix_messages)
        }
        NativeApi::Responses => {
            request.validate_for_raw_responses_passthrough()?;
            let mut prefix_items = Vec::new();
            for entry in &history_entries {
                prefix_items.extend(reconstruct::reconstruct_turn_items(pool, entry).await?);
                let artifact = assistant_artifacts.get(&entry.event_id).ok_or_else(|| {
                    replay_error("missing assistant replay artifact for prior turn")
                })?;
                let replayable_items = reconstruct::replayable_output_items(
                    &persisted_output_items(&artifact.message_json)?,
                );
                prefix_items.extend(output_items_to_input_items(&replayable_items)?);
            }
            request.to_responses_request_with_prefix(&prefix_items, true, true)
        }
        NativeApi::AnthropicMessages => {
            let mut prefix_items = Vec::new();
            for entry in &history_entries {
                prefix_items.extend(reconstruct::reconstruct_turn_items(pool, entry).await?);
                let artifact = assistant_artifacts.get(&entry.event_id).ok_or_else(|| {
                    replay_error("missing assistant replay artifact for prior turn")
                })?;
                let replayable_items = reconstruct::replayable_output_items(
                    &persisted_output_items(&artifact.message_json)?,
                );
                prefix_items.extend(output_items_to_input_items(&replayable_items)?);
            }
            let translated = request.to_responses_request_with_prefix(&prefix_items, true, true)?;
            responses_request_to_anthropic_messages(&translated)
        }
        NativeApi::Realtime => Err(replay_error(
            "Realtime endpoints do not support responses replay assembly",
        )),
        NativeApi::Auto => Err(replay_error(
            "automatic endpoints must resolve their protocol before responses replay assembly",
        )),
    }
}

pub fn needs_responses_replay(path: &str, body: &[u8]) -> bool {
    path == "/v1/responses"
        && (previous_response_id(body).is_some() || conversation_key(body).is_some())
}

const REPLAY_CHAIN_DEPTH_LIMIT: usize = 32;

fn replay_error(message: impl Into<String>) -> CompatError {
    CompatError::new(StatusCode::BAD_REQUEST, "replay_unavailable", message)
}

fn replay_db_error(err: anyhow::Error) -> CompatError {
    CompatError::new(
        StatusCode::BAD_REQUEST,
        "replay_unavailable",
        format!("failed to load stored replay state: {err}"),
    )
}
