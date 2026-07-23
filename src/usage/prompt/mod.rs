use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const REQUEST_CHAIN_DEPTH_LIMIT: usize = 20;
pub const REQUEST_FULL_BYTES_LIMIT: usize = 256 * 1024;
pub const PROMPT_PREVIEW_TEXT_LIMIT: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptBlockSeed {
    pub role: String,
    pub content_json: Value,
    pub preview_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptMessageRef {
    pub role: String,
    pub block_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPromptRequest {
    pub items: Vec<PromptBlockSeed>,
    pub previous_response_id: Option<String>,
    pub conversation: Option<String>,
    pub normalized_bytes_len: usize,
    pub fingerprint: NormalizedPromptFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPromptFingerprint {
    pub normalized_item_count: i32,
    pub normalized_chain_hash: String,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
}

mod hashing;
mod normalize;
mod render;

pub use hashing::{
    append_delta, derive_conversation_id, fingerprint_prompt_refs, prompt_block_hash,
    prompt_message_refs,
};
pub use normalize::normalize_prompt_request;
pub use render::{RenderedPromptMessage, render_prompt_text};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_chat_request_messages() {
        let body = br#"{"messages":[{"role":"system","content":"be terse"},{"role":"user","content":[{"type":"text","text":"hello"}]}]}"#;
        let normalized = normalize_prompt_request("/v1/chat/completions", body).unwrap();
        assert_eq!(normalized.items.len(), 2);
        let messages = normalized
            .items
            .iter()
            .map(|item| RenderedPromptMessage {
                role: item.role.clone(),
                block_hash: String::new(),
                preview_text: item.preview_text.clone(),
                content_json: item.content_json.clone(),
                same_as_turn: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            render_prompt_text(&messages),
            "system: be terse\nuser: hello"
        );
    }

    #[test]
    fn limits_preview_text_without_truncating_content_json() {
        let long_text = "x".repeat(PROMPT_PREVIEW_TEXT_LIMIT + 128);
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": long_text,
                }
            ]
        })
        .to_string();
        let normalized = normalize_prompt_request("/v1/chat/completions", body.as_bytes()).unwrap();

        assert_eq!(
            normalized.items[0].preview_text.chars().count(),
            PROMPT_PREVIEW_TEXT_LIMIT
        );
        assert_eq!(
            normalized.items[0].content_json["content"].as_str(),
            Some(long_text.as_str())
        );
    }

    #[test]
    fn normalizes_responses_instructions_as_system_turn() {
        let body = br#"{"instructions":"be terse","input":[{"role":"user","content":"hello"}]}"#;
        let normalized = normalize_prompt_request("/v1/responses", body).unwrap();

        assert_eq!(normalized.items[0].role, "system");
        assert_eq!(
            normalized.items[0].content_json["content"].as_str(),
            Some("be terse")
        );
        assert_eq!(normalized.items[1].role, "user");
    }

    #[test]
    fn normalizes_responses_request_input() {
        let body = br#"{"previous_response_id":"resp_1","input":[{"role":"user","content":[{"type":"input_text","text":"hello"}]}]}"#;
        let normalized = normalize_prompt_request("/v1/responses", body).unwrap();
        assert_eq!(normalized.previous_response_id.as_deref(), Some("resp_1"));
        assert_eq!(normalized.conversation, None);
        assert_eq!(normalized.items.len(), 1);
    }

    #[test]
    fn normalizes_responses_request_conversation() {
        let body = br#"{"conversation":"conv_1","input":[{"role":"user","content":"hello"}]}"#;
        let normalized = normalize_prompt_request("/v1/responses", body).unwrap();
        assert_eq!(normalized.conversation.as_deref(), Some("conv_1"));
    }

    #[test]
    fn append_delta_requires_strict_prefix_append() {
        let parent = vec![
            PromptMessageRef {
                role: "system".to_string(),
                block_hash: "a".to_string(),
            },
            PromptMessageRef {
                role: "user".to_string(),
                block_hash: "b".to_string(),
            },
        ];
        let current = vec![
            PromptMessageRef {
                role: "system".to_string(),
                block_hash: "a".to_string(),
            },
            PromptMessageRef {
                role: "user".to_string(),
                block_hash: "b".to_string(),
            },
            PromptMessageRef {
                role: "assistant".to_string(),
                block_hash: "c".to_string(),
            },
        ];
        let delta = append_delta(&parent, &current).unwrap();
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].block_hash, "c");
        assert!(append_delta(&current, &current).is_none());
    }

    #[test]
    fn derives_stable_conversation_id() {
        let a = derive_conversation_id(7, "conv-a");
        let b = derive_conversation_id(7, "conv-a");
        let c = derive_conversation_id(8, "conv-a");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fingerprints_prompt_refs_stably() {
        let refs = vec![
            PromptMessageRef {
                role: "user".to_string(),
                block_hash: "a".to_string(),
            },
            PromptMessageRef {
                role: "assistant".to_string(),
                block_hash: "b".to_string(),
            },
        ];
        let fingerprint = fingerprint_prompt_refs(&refs);
        assert_eq!(fingerprint.normalized_item_count, 2);
        assert_eq!(fingerprint.normalized_first_ref_hash.as_deref(), Some("a"));
        assert_eq!(fingerprint.normalized_last_ref_hash.as_deref(), Some("b"));
        assert!(!fingerprint.normalized_chain_hash.is_empty());
    }

    #[test]
    fn normalizes_codex_thread_identity_fields() {
        let body = br#"{
            "prompt_cache_key":"thread-123",
            "client_metadata":{
                "x-codex-window-id":"thread-123:7",
                "x-codex-installation-id":"install-1"
            },
            "input":"hello"
        }"#;
        let value = serde_json::from_slice::<serde_json::Value>(body).unwrap();
        let prompt_cache_key = value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let window_thread_id = value
            .get("client_metadata")
            .and_then(|json| json.get("x-codex-window-id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| {
                value
                    .rsplit_once(':')
                    .map(|(thread_id, _)| thread_id)
                    .or(Some(value))
            });

        assert_eq!(prompt_cache_key, Some("thread-123"));
        assert_eq!(window_thread_id, Some("thread-123"));
    }
}
