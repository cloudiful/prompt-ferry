pub mod anthropic_compat;
pub mod app;
pub mod auth;
pub mod bridge;
pub mod certs;
pub mod chat_replay;
pub mod cli;
pub mod config;
pub mod db;
pub mod endpoint_models;
pub mod endpoint_protocol;
pub mod ip_acl;
pub mod keys;
pub use prompt_ferry_llm_review as llm_review;
pub mod mcp;
pub use prompt_ferry_runtime_env::naming;
pub mod openai_compat;
pub mod openapi_export;
pub mod raw_payload_store;
pub mod realtime;
pub use prompt_ferry_redact as redact;
#[cfg(test)]
pub(crate) use prompt_ferry_redact::test_support as redact_test_support;
pub use prompt_ferry_redact_upstream as redact_upstream;
pub mod relay;
pub(crate) use prompt_ferry_runtime_env::relay_secrets;
pub mod relay_tls;
pub mod replay_cache;
pub mod response_affinity;
pub mod routing;
pub use prompt_ferry_runtime_env::runtime_env;
pub mod serve;
pub mod standalone_config;
pub mod storage;
pub mod storage_sanitization;
pub(crate) mod stream_text;
pub mod tls;
pub mod upstream_adapter;
pub(crate) mod upstream_error;
pub use prompt_ferry_upstream_presets as upstream_presets;
pub mod usage;
pub mod worker;
pub mod worker_admin;

pub use bridge::crypto as bridge_crypto;
pub use bridge::protocol;
pub use bridge::wire as bridge_wire;
pub use usage::logging as worker_usage;
pub use usage::prompt as usage_prompt;
pub use worker_admin::state as worker_admin_state;
pub use worker_admin::types as worker_admin_types;

// Issue #384 Phase 1: `upstream_presets` moved to the
// `prompt-ferry-upstream-presets` leaf crate with decoupled enum duplicates,
// so the root `db`/`config` types convert here. Each match is exhaustive so
// a variant drift on either side fails to compile.
impl From<db::EndpointProvider> for upstream_presets::EndpointProvider {
    fn from(value: db::EndpointProvider) -> Self {
        match value {
            db::EndpointProvider::Generic => Self::Generic,
            db::EndpointProvider::Minimax => Self::Minimax,
            db::EndpointProvider::CommandCode => Self::CommandCode,
            db::EndpointProvider::OpencodeGo => Self::OpencodeGo,
            db::EndpointProvider::OpenRouter => Self::OpenRouter,
            db::EndpointProvider::Glm => Self::Glm,
            db::EndpointProvider::DeepSeek => Self::DeepSeek,
        }
    }
}

impl From<db::EndpointRegion> for upstream_presets::EndpointRegion {
    fn from(value: db::EndpointRegion) -> Self {
        match value {
            db::EndpointRegion::Cn => Self::Cn,
            db::EndpointRegion::Global => Self::Global,
        }
    }
}

impl From<config::NativeApi> for upstream_presets::NativeApi {
    fn from(value: config::NativeApi) -> Self {
        match value {
            config::NativeApi::Auto => Self::Auto,
            config::NativeApi::AnthropicMessages => Self::AnthropicMessages,
            config::NativeApi::Chat => Self::Chat,
            config::NativeApi::Responses => Self::Responses,
            config::NativeApi::Realtime => Self::Realtime,
        }
    }
}
