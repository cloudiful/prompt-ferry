use super::PromptConversationResolution;
use crate::{db, usage_prompt::derive_conversation_id, worker_admin::AdminState};
use anyhow::Result;

use super::super::request_assembly::BufferedBridgeRequest;

pub(super) async fn resolve_prompt_conversation(
    state: &AdminState,
    request: &BufferedBridgeRequest,
    user_id: Option<i64>,
    previous_response_id: Option<&str>,
    provider_conversation_key: Option<&str>,
    session_header_id: Option<&str>,
    codex_thread_key: Option<&str>,
) -> Result<Option<PromptConversationResolution>> {
    if request.path == "/v1/responses" {
        if let Some(codex_thread_key) = codex_thread_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let conversation_id =
                derive_conversation_id(user_id.unwrap_or_default(), codex_thread_key);
            let latest = db::latest_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?;
            let replay_parent = db::latest_replayable_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?;
            let next_seq_seed = latest
                .as_ref()
                .and_then(|entry| entry.conversation_seq)
                .unwrap_or(0)
                + 1;
            let conversation_seq =
                db::allocate_conversation_seq(&state.pool, conversation_id, next_seq_seed).await?;
            return Ok(Some(PromptConversationResolution {
                conversation_id,
                parent_event_id: replay_parent.as_ref().map(|entry| entry.event_id),
                endpoint_id: replay_parent.as_ref().and_then(|entry| entry.endpoint_id),
                conversation_seq,
                source: "codex_thread_key",
            }));
        }

        if let Some(previous_response_id) = previous_response_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && let Some(parent) = db::get_usage_event_locator_by_provider_response_id(
                &state.pool,
                user_id,
                previous_response_id,
            )
            .await?
        {
            let conversation_id = parent.conversation_id.unwrap_or_else(uuid::Uuid::new_v4);
            let latest = if parent.conversation_id.is_some() {
                db::latest_usage_event_locator_by_conversation(
                    &state.pool,
                    user_id,
                    conversation_id,
                )
                .await?
            } else {
                None
            };
            let next_seq_seed = latest
                .as_ref()
                .and_then(|entry| entry.conversation_seq)
                .or(parent.conversation_seq)
                .unwrap_or(1)
                + 1;
            let conversation_seq =
                db::allocate_conversation_seq(&state.pool, conversation_id, next_seq_seed).await?;
            return Ok(Some(PromptConversationResolution {
                conversation_id,
                parent_event_id: Some(parent.event_id),
                endpoint_id: parent.endpoint_id,
                conversation_seq,
                source: "explicit_previous_response_id",
            }));
        }

        if let Some(provider_conversation_key) = provider_conversation_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let existing = db::latest_usage_event_locator_by_provider_conversation_key(
                &state.pool,
                user_id,
                provider_conversation_key,
            )
            .await?;
            let conversation_id = existing
                .as_ref()
                .and_then(|entry| entry.conversation_id)
                .unwrap_or_else(|| {
                    derive_conversation_id(user_id.unwrap_or_default(), provider_conversation_key)
                });
            let latest = db::latest_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?
            .or(existing);
            let replay_parent = db::latest_replayable_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?;
            let next_seq_seed = latest
                .as_ref()
                .and_then(|entry| entry.conversation_seq)
                .unwrap_or(0)
                + 1;
            let conversation_seq =
                db::allocate_conversation_seq(&state.pool, conversation_id, next_seq_seed).await?;
            return Ok(Some(PromptConversationResolution {
                conversation_id,
                parent_event_id: replay_parent.as_ref().map(|entry| entry.event_id),
                endpoint_id: replay_parent.as_ref().and_then(|entry| entry.endpoint_id),
                conversation_seq,
                source: "explicit_conversation",
            }));
        }

        if let Some(session_header_id) = session_header_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let conversation_id =
                derive_conversation_id(user_id.unwrap_or_default(), session_header_id);
            let latest = db::latest_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?;
            let replay_parent = db::latest_replayable_usage_event_locator_by_conversation(
                &state.pool,
                user_id,
                conversation_id,
            )
            .await?;
            let next_seq_seed = latest
                .as_ref()
                .and_then(|entry| entry.conversation_seq)
                .unwrap_or(0)
                + 1;
            let conversation_seq =
                db::allocate_conversation_seq(&state.pool, conversation_id, next_seq_seed).await?;
            return Ok(Some(PromptConversationResolution {
                conversation_id,
                parent_event_id: replay_parent.as_ref().map(|entry| entry.event_id),
                endpoint_id: replay_parent.as_ref().and_then(|entry| entry.endpoint_id),
                conversation_seq,
                source: "session_header",
            }));
        }

        return Ok(None);
    }

    Ok(None)
}
