use std::collections::HashMap;

use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db,
    openai_compat::{persisted_assistant_message, translate_input},
    usage_prompt::PromptMessageRef,
};

use super::{AssistantArtifact, replay_db_error, replay_error};

pub(super) async fn reconstruct_turn_messages(
    pool: &PgPool,
    entry: &db::UsageEventChainEntry,
) -> Result<Vec<Value>, crate::openai_compat::CompatError> {
    translate_input(&Value::Array(reconstruct_turn_items(pool, entry).await?))
}

pub(super) async fn reconstruct_turn_items(
    pool: &PgPool,
    entry: &db::UsageEventChainEntry,
) -> Result<Vec<Value>, crate::openai_compat::CompatError> {
    let refs = prompt_refs_for_entry(entry)?;
    let blocks = ordered_prompt_blocks(pool, &refs).await?;
    let mut items = match entry.path.as_str() {
        "/v1/responses" => blocks
            .into_iter()
            .map(|block| block.content_json)
            .collect::<Vec<_>>(),
        "/v1/chat/completions" => blocks
            .into_iter()
            .map(|block| {
                let mut message = block.content_json.as_object().cloned().unwrap_or_default();
                message.insert("role".to_string(), Value::String(block.role));
                Value::Object(message)
            })
            .collect::<Vec<_>>(),
        _ => {
            return Err(replay_error(format!(
                "stored conversation contains unsupported path `{}` for replay",
                entry.path
            )));
        }
    };
    strip_leading_system_items(&mut items);
    Ok(items)
}

pub(in crate::chat_replay) fn replayable_output_items(output_items: &[Value]) -> Vec<Value> {
    output_items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) != Some("reasoning"))
        .cloned()
        .collect()
}

pub(in crate::chat_replay) fn should_replay_reasoning(
    current_request_model: Option<&str>,
    parent_model: Option<&str>,
    route_base_url: &str,
    artifacts: &HashMap<i64, AssistantArtifact>,
) -> bool {
    let history_has_reasoning = artifacts
        .values()
        .any(|artifact| artifact.has_reasoning_content);
    if !history_has_reasoning {
        return false;
    }
    current_request_model.is_some_and(is_deepseek_model)
        || current_request_model.is_some_and(is_minimax_model)
        || parent_model.is_some_and(is_deepseek_model)
        || parent_model.is_some_and(is_minimax_model)
        || route_base_url.to_ascii_lowercase().contains("deepseek")
        || route_base_url.to_ascii_lowercase().contains("minimax")
}

pub(in crate::chat_replay) fn replay_assistant_message(
    message_json: &Value,
    include_reasoning: bool,
) -> Result<Value, crate::openai_compat::CompatError> {
    let normalized = persisted_assistant_message(message_json)?;
    reject_unsupported_replay_semantics(&normalized)?;
    let mut message = normalized.as_object().cloned().unwrap_or_default();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if !include_reasoning {
        message.remove("reasoning_content");
        message.remove("reasoning_details");
    }
    if !message.contains_key("content") {
        message.insert("content".to_string(), Value::Null);
    }
    Ok(Value::Object(message))
}

fn prompt_refs_for_entry(
    entry: &db::UsageEventChainEntry,
) -> Result<Vec<PromptMessageRef>, crate::openai_compat::CompatError> {
    match entry.request_storage_mode.as_str() {
        "full" => entry
            .request_full_json
            .as_ref()
            .ok_or_else(|| replay_error("stored request checkpoint is missing replay refs"))
            .and_then(|value| db::decode_prompt_message_refs(value).map_err(replay_db_error)),
        "append_delta" => entry
            .request_delta_json
            .as_ref()
            .ok_or_else(|| replay_error("stored request delta is missing replay refs"))
            .and_then(|value| db::decode_prompt_message_refs(value).map_err(replay_db_error)),
        other => Err(replay_error(format!(
            "stored request mode `{other}` is not supported for replay"
        ))),
    }
}

async fn ordered_prompt_blocks(
    pool: &PgPool,
    refs: &[PromptMessageRef],
) -> Result<Vec<db::UsagePromptBlock>, crate::openai_compat::CompatError> {
    let hashes = refs
        .iter()
        .map(|reference| reference.block_hash.clone())
        .collect::<Vec<_>>();
    let blocks = db::get_usage_prompt_blocks(pool, &hashes)
        .await
        .map_err(replay_db_error)?;
    let block_map = blocks
        .into_iter()
        .map(|block| {
            let _created_at = block.created_at;
            (block.block_hash.clone(), block)
        })
        .collect::<HashMap<_, _>>();
    refs.iter()
        .map(|reference| {
            block_map
                .get(&reference.block_hash)
                .cloned()
                .ok_or_else(|| replay_error("stored prompt block is missing for replay"))
        })
        .collect()
}

fn strip_leading_system_items(items: &mut Vec<Value>) {
    while items
        .first()
        .and_then(Value::as_object)
        .and_then(|object| object.get("role").and_then(Value::as_str))
        == Some("system")
    {
        items.remove(0);
    }
}

fn reject_unsupported_replay_semantics(
    message_json: &Value,
) -> Result<(), crate::openai_compat::CompatError> {
    let Some(object) = message_json.as_object() else {
        return Ok(());
    };
    for key in ["phase", "refusal"] {
        if object.get(key).is_some_and(has_meaningful_value) {
            return Err(replay_error(format!(
                "stored replay state contains unsupported semantics such as `{key}`"
            )));
        }
    }
    Ok(())
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

fn is_deepseek_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("deepseek-")
}

fn is_minimax_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("minimax-")
}
