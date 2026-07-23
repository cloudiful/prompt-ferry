use serde_json::Value;

use crate::stream_text::Utf8LineDecoder;

mod capture;
pub mod logging;
pub mod prompt;
mod request;
mod text;

pub use capture::{TokenUsage, UsageCapture};
pub use logging::{UsageLog, record_usage_event};
pub use prompt::{
    NormalizedPromptFingerprint, PromptBlockSeed, PromptMessageRef, REQUEST_CHAIN_DEPTH_LIMIT,
    REQUEST_FULL_BYTES_LIMIT, append_delta, derive_conversation_id, normalize_prompt_request,
    prompt_block_hash, prompt_message_refs,
};
pub use request::{extract_request_prompt, model_from_body, rewrite_model_in_body, upstream_body};
pub use text::{extract_usage, truncate_chars};
