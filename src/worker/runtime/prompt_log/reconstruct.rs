use super::ReconstructedPromptChain;
use crate::{
    db, redact,
    usage_prompt::{PromptBlockSeed, REQUEST_CHAIN_DEPTH_LIMIT},
};
use anyhow::Result;
use serde_json::Value;

pub(super) async fn reconstruct_prompt_chain(
    pool: &sqlx::PgPool,
    entry: &db::UsageEventChainEntry,
) -> Result<Option<ReconstructedPromptChain>> {
    let mut current = entry.clone();
    let mut segments = Vec::new();
    let mut depth = 0;
    loop {
        depth += 1;
        if depth > REQUEST_CHAIN_DEPTH_LIMIT {
            return Ok(None);
        }
        match current.request_storage_mode.as_str() {
            "full" => {
                let Some(value) = current.request_full_json.as_ref() else {
                    return Ok(None);
                };
                segments.push(db::decode_prompt_message_refs(value)?);
                break;
            }
            "append_delta" => {
                let Some(value) = current.request_delta_json.as_ref() else {
                    return Ok(None);
                };
                segments.push(db::decode_prompt_message_refs(value)?);
                let Some(parent_event_id) = current.parent_event_id else {
                    return Ok(None);
                };
                let Some(parent) = db::get_usage_event_chain_entry(pool, parent_event_id).await?
                else {
                    return Ok(None);
                };
                current = parent;
            }
            _ => return Ok(None),
        }
    }
    segments.reverse();
    let mut refs = Vec::new();
    for segment in segments {
        refs.extend(segment);
    }
    Ok(Some(ReconstructedPromptChain { refs, depth }))
}

pub(super) fn redact_prompt_item(item: PromptBlockSeed, user_id: Option<i64>) -> PromptBlockSeed {
    PromptBlockSeed {
        role: item.role,
        content_json: redact_prompt_value(&item.content_json, user_id),
        preview_text: redact::redact_text_for_user(&item.preview_text, user_id),
    }
}

fn redact_prompt_value(value: &Value, user_id: Option<i64>) -> Value {
    match value {
        Value::String(text) => Value::String(redact::redact_text_for_user(text, user_id)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_prompt_value(item, user_id))
                .collect(),
        ),
        Value::Object(object) => {
            let mut next = serde_json::Map::new();
            for (key, value) in object {
                next.insert(key.clone(), redact_prompt_value(value, user_id));
            }
            Value::Object(next)
        }
        _ => value.clone(),
    }
}
