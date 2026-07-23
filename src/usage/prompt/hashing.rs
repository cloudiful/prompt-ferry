use crate::naming::CONVERSATION_HASH_NAMESPACE;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{NormalizedPromptFingerprint, PromptBlockSeed, PromptMessageRef};

pub fn prompt_message_refs(items: &[PromptBlockSeed]) -> Vec<PromptMessageRef> {
    items
        .iter()
        .map(|item| PromptMessageRef {
            role: item.role.clone(),
            block_hash: prompt_block_hash(&item.role, &item.content_json),
        })
        .collect()
}

pub fn prompt_block_hash(role: &str, content_json: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(role.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(content_json).unwrap_or_default());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn derive_conversation_id(user_id: i64, hint: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(CONVERSATION_HASH_NAMESPACE);
    hasher.update([0]);
    hasher.update(user_id.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(hint.trim().as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hasher.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub fn append_delta(
    parent: &[PromptMessageRef],
    current: &[PromptMessageRef],
) -> Option<Vec<PromptMessageRef>> {
    if current.len() <= parent.len() || !current.starts_with(parent) {
        return None;
    }
    Some(current[parent.len()..].to_vec())
}

pub fn fingerprint_prompt_refs(refs: &[PromptMessageRef]) -> NormalizedPromptFingerprint {
    let mut hasher = Sha256::new();
    for item in refs {
        hasher.update(item.role.as_bytes());
        hasher.update([0]);
        hasher.update(item.block_hash.as_bytes());
        hasher.update([0xff]);
    }
    let normalized_chain_hash = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    NormalizedPromptFingerprint {
        normalized_item_count: refs.len().try_into().unwrap_or(i32::MAX),
        normalized_chain_hash,
        normalized_first_ref_hash: refs.first().map(|item| item.block_hash.clone()),
        normalized_last_ref_hash: refs.last().map(|item| item.block_hash.clone()),
    }
}
