use std::collections::HashMap;

use sqlx::PgPool;
use tracing::warn;

use crate::{chat_replay::fallback_text_artifact, db, replay_cache::ReplayCache};

use crate::chat_replay::AssistantArtifact;

use super::{REPLAY_CHAIN_DEPTH_LIMIT, replay_db_error, replay_error};

pub(super) async fn load_history_entries(
    pool: &PgPool,
    replay_cache: &ReplayCache,
    parent: &db::UsageEventChainEntry,
) -> Result<Vec<db::UsageEventChainEntry>, crate::openai_compat::CompatError> {
    if let Some(conversation_id) = parent.conversation_id
        && let Some(target_seq) = parent.conversation_seq
    {
        let cache_hit = match replay_cache.get_snapshot(conversation_id).await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                warn!(error = %err, conversation_id = %conversation_id, "failed to load replay valkey snapshot");
                None
            }
        };
        if let Some(snapshot) = cache_hit
            && snapshot.conversation_seq >= target_seq
        {
            match load_history_tail_from_snapshot(pool, parent, Some(snapshot.base_event_id)).await
            {
                Ok(tail) => return Ok(tail),
                Err(err) => {
                    warn!(
                        conversation_id = %snapshot.conversation_id,
                        snapshot_seq = snapshot.conversation_seq,
                        snapshot_base_event_id = snapshot.base_event_id,
                        target_seq,
                        error_code = %err.code,
                        error_message = %err.message,
                        "replay valkey snapshot was unusable; falling back to persisted snapshot"
                    );
                }
            }
        }
        let persisted = db::replay_snapshot_before_or_at_seq(pool, conversation_id, target_seq)
            .await
            .map_err(replay_db_error)?;
        if let Some(snapshot) = persisted {
            let tail =
                load_history_tail_from_snapshot(pool, parent, Some(snapshot.base_event_id)).await?;
            return Ok(tail);
        }
    }

    load_full_history_entries(pool, parent).await
}

pub(super) async fn load_assistant_artifacts(
    pool: &PgPool,
    entries: &[db::UsageEventChainEntry],
) -> Result<HashMap<i64, AssistantArtifact>, crate::openai_compat::CompatError> {
    let event_ids = entries
        .iter()
        .map(|entry| entry.event_id)
        .collect::<Vec<_>>();
    let artifacts = db::get_usage_assistant_artifacts(pool, &event_ids)
        .await
        .map_err(replay_db_error)?;
    let mut map = HashMap::new();
    for artifact in artifacts {
        let _created_at = artifact.created_at;
        map.insert(
            artifact.event_id,
            AssistantArtifact {
                message_json: artifact.message_json,
                has_reasoning_content: artifact.has_reasoning_content,
                has_tool_calls: artifact.has_tool_calls,
            },
        );
    }
    for entry in entries {
        if map.contains_key(&entry.event_id) {
            continue;
        }
        let Some(artifact) = fallback_artifact_for_entry(entry) else {
            return Err(replay_error(
                "stored replay state is missing and this turn cannot be reconstructed from stored response text",
            ));
        };
        db::upsert_usage_assistant_artifact(
            pool,
            db::UsageAssistantArtifactCreate {
                event_id: entry.event_id,
                message_json: artifact.message_json.clone(),
                has_reasoning_content: artifact.has_reasoning_content,
                has_tool_calls: artifact.has_tool_calls,
            },
        )
        .await
        .map_err(replay_db_error)?;
        map.insert(entry.event_id, artifact);
    }
    Ok(map)
}

pub(in crate::chat_replay) fn fallback_artifact_for_entry(
    entry: &db::UsageEventChainEntry,
) -> Option<AssistantArtifact> {
    fallback_text_artifact(entry.response_prompt.as_deref().unwrap_or_default())
}

async fn load_full_history_entries(
    pool: &PgPool,
    parent: &db::UsageEventChainEntry,
) -> Result<Vec<db::UsageEventChainEntry>, crate::openai_compat::CompatError> {
    let mut current = parent.clone();
    let mut entries = Vec::new();
    loop {
        entries.push(current.clone());
        if entries.len() > REPLAY_CHAIN_DEPTH_LIMIT {
            return Err(replay_error(
                "stored conversation is too deep to replay safely",
            ));
        }
        let Some(parent_event_id) = current.parent_event_id else {
            break;
        };
        let Some(parent_entry) = db::get_usage_event_chain_entry(pool, parent_event_id)
            .await
            .map_err(replay_db_error)?
        else {
            return Err(replay_error(
                "stored conversation chain is incomplete and cannot be replayed",
            ));
        };
        current = parent_entry;
    }
    entries.reverse();
    Ok(entries)
}

async fn load_history_tail_from_snapshot(
    pool: &PgPool,
    parent: &db::UsageEventChainEntry,
    base_event_id: Option<i64>,
) -> Result<Vec<db::UsageEventChainEntry>, crate::openai_compat::CompatError> {
    let mut current = parent.clone();
    let mut entries = Vec::new();
    loop {
        entries.push(current.clone());
        if entries.len() > REPLAY_CHAIN_DEPTH_LIMIT {
            return Err(replay_error(
                "stored conversation is too deep to replay safely",
            ));
        }
        if Some(current.event_id) == base_event_id {
            break;
        }
        let Some(parent_event_id) = current.parent_event_id else {
            break;
        };
        let Some(parent_entry) = db::get_usage_event_chain_entry(pool, parent_event_id)
            .await
            .map_err(replay_db_error)?
        else {
            return Err(replay_error(
                "stored conversation chain is incomplete and cannot be replayed",
            ));
        };
        current = parent_entry;
    }
    entries.reverse();
    Ok(entries)
}
