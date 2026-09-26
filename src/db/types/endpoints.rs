use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::config::{NativeApi, NativeApiSource};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProvider {
    Generic,
    Minimax,
    CommandCode,
    OpencodeGo,
    // `OpenRouter` snake_cases to `open_router` by default; the admin API
    // contract (issue #203) uses the single-token `openrouter`, so rename
    // explicitly while keeping snake_case for the other variants.
    #[serde(rename = "openrouter")]
    OpenRouter,
    // `Glm` is a non-MiniMax provider (issue #230) — same as the
    // CommandCode/OpencodeGo/OpenRouter snake_case convention. It carries no
    // `provider_region` (the 0076 region CHECK rejects any non-NULL value)
    // and never gains the MiniMax builtin MCP privilege.
    #[serde(rename = "glm")]
    Glm,
    // DeepSeek (issue #287) is the seventh provider. Like the other
    // non-MiniMax presets it carries no region and no builtin MCP privilege;
    // `DeepSeek` snake_cases to `deep_seek`, so rename to the single-token
    // `deepseek` used by the admin API contract and the 0079 CHECK.
    #[serde(rename = "deepseek")]
    DeepSeek,
    // OpenAI Platform (issue #589) is the eighth provider. It carries no
    // region and no builtin MCP privilege; `OpenAi` snake_cases to `open_ai`,
    // so rename to the single-token `openai` used by the admin API contract
    // and the provider CHECKs.
    #[serde(rename = "openai")]
    OpenAi,
}

impl Default for EndpointProvider {
    fn default() -> Self {
        Self::Generic
    }
}

impl EndpointProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Minimax => "minimax",
            Self::CommandCode => "command_code",
            Self::OpencodeGo => "opencode_go",
            Self::OpenRouter => "openrouter",
            Self::Glm => "glm",
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "minimax" => Self::Minimax,
            "command_code" => Self::CommandCode,
            "opencode_go" => Self::OpencodeGo,
            "openrouter" => Self::OpenRouter,
            "glm" => Self::Glm,
            "deepseek" => Self::DeepSeek,
            "openai" => Self::OpenAi,
            _ => Self::Generic,
        }
    }

    pub fn from_optional(value: Option<&str>) -> Self {
        match value {
            Some("minimax") => Self::Minimax,
            Some("command_code") => Self::CommandCode,
            Some("opencode_go") => Self::OpencodeGo,
            Some("openrouter") => Self::OpenRouter,
            Some("glm") => Self::Glm,
            Some("deepseek") => Self::DeepSeek,
            Some("openai") => Self::OpenAi,
            _ => Self::Generic,
        }
    }

    /// Issue #562: whether a tool-bearing thinking turn must pass the parent
    /// reasoning back to this upstream. Only DeepSeek is confirmed by a 400
    /// fingerprint (`reasoning_content in the thinking mode must be passed
    /// back`, issue #556), so the pre-flight thinking downgrade gates on this
    /// bit and every other upstream forwards the requested thinking unchanged.
    /// The table stays conservative: a new provider needs its own fingerprint
    /// evidence before it can flip this.
    pub fn requires_reasoning_echo(self) -> bool {
        matches!(self, Self::DeepSeek)
    }

    /// Issue #599 R2a: whether this provider may carry a ChatGPT subscription
    /// (OAuth) credential. Only OpenAI endpoints have the plan axis; every
    /// other provider is API-key-only and rejects OAuth token storage.
    pub fn supports_chatgpt_subscription_plan(self) -> bool {
        matches!(self, Self::OpenAi)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRegion {
    Cn,
    Global,
}

impl EndpointRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Global => "global",
        }
    }

    fn from_str(value: Option<&str>) -> Option<Self> {
        match value {
            Some("cn") => Some(Self::Cn),
            Some("global") => Some(Self::Global),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointPlan {
    #[default]
    PlatformApiKey,
    ChatgptSubscription,
}

impl EndpointPlan {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlatformApiKey => "platform_api_key",
            Self::ChatgptSubscription => "chatgpt_subscription",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "chatgpt_subscription" => Self::ChatgptSubscription,
            _ => Self::PlatformApiKey,
        }
    }

    pub fn from_optional(value: Option<&str>) -> Self {
        match value {
            Some("chatgpt_subscription") => Self::ChatgptSubscription,
            _ => Self::PlatformApiKey,
        }
    }

    /// Issue #599 R2a: effective plan for an endpoint. The plan axis lives on
    /// the OpenAI provider only and is derived server-side from the stored
    /// OAuth token: `chatgpt_subscription` iff a token is stored on an OpenAI
    /// endpoint, `platform_api_key` otherwise. Client-supplied plan values are
    /// validated by `validate_endpoint_plan`, never trusted blindly.
    pub fn resolve(provider: EndpointProvider, has_oauth_token: bool) -> Self {
        if has_oauth_token && provider.supports_chatgpt_subscription_plan() {
            Self::ChatgptSubscription
        } else {
            Self::PlatformApiKey
        }
    }
}

/// Issue #599 R2a: per-endpoint ChatGPT subscription OAuth token. Server-side
/// only: the secrets never serialize (no `Serialize` impl on purpose) and the
/// custom `Debug` redacts them, mirroring the `api_key` redaction. The refresh
/// token lives only in the encrypted store columns, never in logs or
/// responses.
#[derive(Clone)]
pub struct EndpointOAuthToken {
    pub endpoint_id: uuid::Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for EndpointOAuthToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Both secrets render as byte counts so a token can never leak
        // through logs while remaining identifiable by endpoint and expiry.
        formatter
            .debug_struct("EndpointOAuthToken")
            .field("endpoint_id", &self.endpoint_id)
            .field("access_token", &redacted_secret_len(&self.access_token))
            .field("refresh_token", &redacted_secret_len(&self.refresh_token))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Issue #599 R2a: secrets for storing (or refreshing) an endpoint OAuth
/// token. `None` at the repository boundary clears the stored token instead
/// (NULL convention); this struct only carries fresh secrets inward.
#[derive(Clone)]
pub struct EndpointOAuthTokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for EndpointOAuthTokenSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EndpointOAuthTokenSet")
            .field("access_token", &redacted_secret_len(&self.access_token))
            .field("refresh_token", &redacted_secret_len(&self.refresh_token))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

fn redacted_secret_len(value: &str) -> String {
    format!("[REDACTED; {} bytes]", value.len())
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinimaxServiceTier {
    #[default]
    Standard,
    Priority,
}

impl MinimaxServiceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Priority => "priority",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "priority" => Self::Priority,
            _ => Self::Standard,
        }
    }

    pub fn from_optional(value: Option<&str>) -> Self {
        match value {
            Some("priority") => Self::Priority,
            _ => Self::Standard,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct EndpointApiKey {
    pub key_id: uuid::Uuid,
    pub endpoint_id: uuid::Uuid,
    pub key_label: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub position: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EndpointApiKeySelection {
    pub key_id: Option<uuid::Uuid>,
    pub key_label: Option<String>,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ProviderEndpointRow {
    pub endpoint_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: String,
    pub provider_region: Option<String>,
    pub service_tier: Option<String>,
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    // Issue #368 Phase A: outbound proxy default (full URL with optional
    // userinfo). NULL means direct. Plaintext on PG mirrors `api_key`;
    // SQLite uses the envelope columns (standalone 0018).
    #[serde(skip_serializing)]
    pub proxy_url: Option<String>,
    // Issue #392 Phase K: endpoint default windows (`active_windows` TEXT
    // NULL). Effective windows resolve as target-nonempty else endpoint
    // else all-day. Carried as stored string for routing inheritance.
    pub active_windows: Option<String>,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderEndpoint {
    pub endpoint_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    /// Issue #599 R2a: upstream plan axis (`platform_api_key` |
    /// `chatgpt_subscription`). Derived server-side from the stored OAuth
    /// token (see `EndpointPlan::resolve`); `serde(default)` keeps the
    /// contract backward-compatible, mirroring `has_proxy_url`.
    #[serde(default)]
    pub plan: EndpointPlan,
    #[serde(default)]
    pub service_tier: MinimaxServiceTier,
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    // Issue #368 Phase A: never echoed (mirrors `api_key` redaction).
    #[serde(skip_serializing)]
    pub proxy_url: Option<String>,
    /// Issue #599 R2a: response-side saved-OAuth-token indicator. `true` when
    /// a ChatGPT OAuth token is stored; the secrets themselves are never
    /// echoed, mirroring `has_proxy_url`.
    #[serde(default)]
    pub has_oauth_token: bool,
    /// Issue #368 Phase C (P2): response-side saved-proxy indicator.
    /// `true` when a proxy URL is stored; the secret itself is never echoed.
    #[serde(default)]
    pub has_proxy_url: bool,
    /// Issue #392 Phase K: endpoint default windows (`HH:MM` pairs).
    /// Empty means all-day; empty target inherits this value.
    #[serde(default)]
    pub active_windows: Vec<crate::db::types::ActiveWindow>,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub api_keys: Vec<EndpointApiKey>,
}

impl From<ProviderEndpointRow> for ProviderEndpoint {
    fn from(value: ProviderEndpointRow) -> Self {
        // Issue #368 Phase C (P2): derive the saved-proxy indicator from the
        // stored value; the secret itself stays `skip_serializing`.
        let has_proxy_url = value
            .proxy_url
            .as_deref()
            .is_some_and(|raw| !raw.trim().is_empty());
        // Issue #392 Phase K: stored JSON array; corrupt reads as all-day
        // for display (routing fails closed separately).
        let active_windows = value
            .active_windows
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        Self {
            endpoint_id: value.endpoint_id,
            scope: value.scope,
            owner_user_id: value.owner_user_id,
            name: value.name,
            provider: EndpointProvider::from_str(&value.provider),
            provider_region: EndpointRegion::from_str(value.provider_region.as_deref()),
            // Issue #599 R2a: the endpoint row SELECTs predate the OAuth
            // token table, so rows start on the platform plan without a
            // token; the unified repository enriches both fields from token
            // presence before serving admin responses.
            plan: EndpointPlan::default(),
            service_tier: MinimaxServiceTier::from_optional(value.service_tier.as_deref()),
            base_url: value.base_url,
            native_api: value.native_api,
            native_api_source: value.native_api_source,
            api_key: value.api_key,
            proxy_url: value.proxy_url,
            has_proxy_url,
            has_oauth_token: false,
            active_windows,
            key_lb_enabled: value.key_lb_enabled,
            enabled: value.enabled,
            mcp_enabled: value.mcp_enabled,
            created_at: value.created_at,
            updated_at: value.updated_at,
            api_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EndpointCreate {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    #[serde(default)]
    pub service_tier: MinimaxServiceTier,
    pub base_url: String,
    pub native_api: NativeApi,
    pub native_api_source: NativeApiSource,
    pub api_key: String,
    pub api_keys: Vec<EndpointApiKeyCreate>,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    // Issue #368 Phase A: plaintext proxy default for the PG write path
    // (SQLite encrypts via the 0018 envelope). `None`/empty means direct.
    #[serde(default)]
    pub proxy_url: Option<String>,
    // Issue #392 Phase K: endpoint default windows. `None` (omitted) means
    // keep on PATCH / all-day on create; `Some([])` means all-day;
    // `Some([...])` replaces after HH:MM validation (sorted normalize).
    #[serde(default)]
    pub active_windows: Option<Vec<crate::db::types::ActiveWindow>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EndpointApiKeyCreate {
    pub key_label: String,
    pub api_key: String,
    pub position: i32,
    pub enabled: bool,
    pub key_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EndpointPage {
    pub total: i64,
    pub endpoints: Vec<ProviderEndpoint>,
    pub first: i64,
    pub rows: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_code_provider_round_trips_as_snake_case() {
        assert_eq!(EndpointProvider::CommandCode.as_str(), "command_code");
        assert_eq!(
            EndpointProvider::from_str("command_code"),
            EndpointProvider::CommandCode
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("command_code")),
            EndpointProvider::CommandCode
        );
        // Serde uses snake_case, matching the admin API contract.
        let serialized = serde_json::to_value(EndpointProvider::CommandCode)
            .expect("serialize command_code provider");
        assert_eq!(serialized, serde_json::json!("command_code"));
        let deserialized: EndpointProvider =
            serde_json::from_value(serde_json::json!("command_code"))
                .expect("deserialize command_code provider");
        assert_eq!(deserialized, EndpointProvider::CommandCode);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn opencode_go_provider_round_trips_as_snake_case() {
        assert_eq!(EndpointProvider::OpencodeGo.as_str(), "opencode_go");
        assert_eq!(
            EndpointProvider::from_str("opencode_go"),
            EndpointProvider::OpencodeGo
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("opencode_go")),
            EndpointProvider::OpencodeGo
        );
        // Serde uses snake_case, matching the admin API contract.
        let serialized = serde_json::to_value(EndpointProvider::OpencodeGo)
            .expect("serialize opencode_go provider");
        assert_eq!(serialized, serde_json::json!("opencode_go"));
        let deserialized: EndpointProvider =
            serde_json::from_value(serde_json::json!("opencode_go"))
                .expect("deserialize opencode_go provider");
        assert_eq!(deserialized, EndpointProvider::OpencodeGo);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn openrouter_provider_round_trips_as_snake_case() {
        assert_eq!(EndpointProvider::OpenRouter.as_str(), "openrouter");
        assert_eq!(
            EndpointProvider::from_str("openrouter"),
            EndpointProvider::OpenRouter
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("openrouter")),
            EndpointProvider::OpenRouter
        );
        // Serde uses snake_case, matching the admin API contract.
        let serialized = serde_json::to_value(EndpointProvider::OpenRouter)
            .expect("serialize openrouter provider");
        assert_eq!(serialized, serde_json::json!("openrouter"));
        let deserialized: EndpointProvider =
            serde_json::from_value(serde_json::json!("openrouter"))
                .expect("deserialize openrouter provider");
        assert_eq!(deserialized, EndpointProvider::OpenRouter);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn glm_provider_round_trips_as_snake_case() {
        // Issue #230: GLM is the Zhipu Coding Plan provider. It serializes
        // to the single-token `glm` (matching the 0076/0015 migration
        // CHECKs and the admin API contract), and `from_optional(None)`
        // must keep the legacy generic fallback for legacy rows.
        assert_eq!(EndpointProvider::Glm.as_str(), "glm");
        assert_eq!(EndpointProvider::from_str("glm"), EndpointProvider::Glm);
        assert_eq!(
            EndpointProvider::from_optional(Some("glm")),
            EndpointProvider::Glm
        );
        let serialized =
            serde_json::to_value(EndpointProvider::Glm).expect("serialize glm provider");
        assert_eq!(serialized, serde_json::json!("glm"));
        let deserialized: EndpointProvider =
            serde_json::from_value(serde_json::json!("glm")).expect("deserialize glm provider");
        assert_eq!(deserialized, EndpointProvider::Glm);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
        assert_eq!(
            EndpointProvider::from_optional(None),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn deepseek_provider_round_trips_as_single_token() {
        // Issue #287: DeepSeek is the seventh provider; it serializes to the
        // single-token `deepseek` (matching the 0079 CHECK and the admin API
        // contract), not the derived `deep_seek`.
        assert_eq!(EndpointProvider::DeepSeek.as_str(), "deepseek");
        assert_eq!(
            EndpointProvider::from_str("deepseek"),
            EndpointProvider::DeepSeek
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("deepseek")),
            EndpointProvider::DeepSeek
        );
        let serialized =
            serde_json::to_value(EndpointProvider::DeepSeek).expect("serialize deepseek provider");
        assert_eq!(serialized, serde_json::json!("deepseek"));
        let deserialized: EndpointProvider = serde_json::from_value(serde_json::json!("deepseek"))
            .expect("deserialize deepseek provider");
        assert_eq!(deserialized, EndpointProvider::DeepSeek);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
        assert_eq!(
            EndpointProvider::from_optional(None),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn openai_provider_round_trips_as_single_token() {
        // Issue #589: OpenAI is the eighth provider; it serializes to the
        // single-token `openai` (matching the provider CHECKs and the admin
        // API contract), not the derived `open_ai`.
        assert_eq!(EndpointProvider::OpenAi.as_str(), "openai");
        assert_eq!(
            EndpointProvider::from_str("openai"),
            EndpointProvider::OpenAi
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("openai")),
            EndpointProvider::OpenAi
        );
        let serialized =
            serde_json::to_value(EndpointProvider::OpenAi).expect("serialize openai provider");
        assert_eq!(serialized, serde_json::json!("openai"));
        let deserialized: EndpointProvider = serde_json::from_value(serde_json::json!("openai"))
            .expect("deserialize openai provider");
        assert_eq!(deserialized, EndpointProvider::OpenAi);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str("legacy-unknown"),
            EndpointProvider::Generic
        );
        assert_eq!(
            EndpointProvider::from_optional(None),
            EndpointProvider::Generic
        );
    }

    #[test]
    fn endpoint_plan_round_trips_as_snake_case() {
        // Issue #599 R2a: the plan axis lives on the OpenAI provider
        // (`platform_api_key | chatgpt_subscription`); it serializes as
        // snake_case and unknown/legacy values fall back to the platform plan.
        assert_eq!(EndpointPlan::PlatformApiKey.as_str(), "platform_api_key");
        assert_eq!(
            EndpointPlan::ChatgptSubscription.as_str(),
            "chatgpt_subscription"
        );
        assert_eq!(EndpointPlan::default(), EndpointPlan::PlatformApiKey);
        assert_eq!(
            EndpointPlan::from_str("chatgpt_subscription"),
            EndpointPlan::ChatgptSubscription
        );
        assert_eq!(
            EndpointPlan::from_str("platform_api_key"),
            EndpointPlan::PlatformApiKey
        );
        assert_eq!(
            EndpointPlan::from_str("legacy-unknown"),
            EndpointPlan::PlatformApiKey
        );
        assert_eq!(
            EndpointPlan::from_optional(Some("chatgpt_subscription")),
            EndpointPlan::ChatgptSubscription
        );
        assert_eq!(
            EndpointPlan::from_optional(None),
            EndpointPlan::PlatformApiKey
        );
        let serialized = serde_json::to_value(EndpointPlan::ChatgptSubscription)
            .expect("serialize chatgpt_subscription plan");
        assert_eq!(serialized, serde_json::json!("chatgpt_subscription"));
        let deserialized: EndpointPlan =
            serde_json::from_value(serde_json::json!("platform_api_key"))
                .expect("deserialize platform_api_key plan");
        assert_eq!(deserialized, EndpointPlan::PlatformApiKey);
    }

    #[test]
    fn endpoint_plan_resolves_from_oauth_presence_on_openai_only() {
        // Issue #599 R2a: the effective plan is derived server-side. A stored
        // token flips an OpenAI endpoint to the subscription plan; every other
        // provider stays on the platform plan even if a token row lingered.
        assert_eq!(
            EndpointPlan::resolve(EndpointProvider::OpenAi, true),
            EndpointPlan::ChatgptSubscription
        );
        for provider in [
            EndpointProvider::Generic,
            EndpointProvider::Minimax,
            EndpointProvider::CommandCode,
            EndpointProvider::OpencodeGo,
            EndpointProvider::OpenRouter,
            EndpointProvider::Glm,
            EndpointProvider::DeepSeek,
            EndpointProvider::OpenAi,
        ] {
            assert_eq!(
                EndpointPlan::resolve(provider, false),
                EndpointPlan::PlatformApiKey,
                "{provider:?} without a token must stay on the platform plan"
            );
        }
        for provider in [
            EndpointProvider::Generic,
            EndpointProvider::Minimax,
            EndpointProvider::CommandCode,
            EndpointProvider::OpencodeGo,
            EndpointProvider::OpenRouter,
            EndpointProvider::Glm,
            EndpointProvider::DeepSeek,
        ] {
            assert_eq!(
                EndpointPlan::resolve(provider, true),
                EndpointPlan::PlatformApiKey,
                "{provider:?} must never resolve to the subscription plan"
            );
        }
    }

    #[test]
    fn only_openai_supports_chatgpt_subscription_plan() {
        // Issue #599 R2a: the plan axis is OpenAI-only; the storage gate and
        // the admin validation share this bit so a future provider needs an
        // explicit opt-in here.
        assert!(EndpointProvider::OpenAi.supports_chatgpt_subscription_plan());
        for provider in [
            EndpointProvider::Generic,
            EndpointProvider::Minimax,
            EndpointProvider::CommandCode,
            EndpointProvider::OpencodeGo,
            EndpointProvider::OpenRouter,
            EndpointProvider::Glm,
            EndpointProvider::DeepSeek,
        ] {
            assert!(
                !provider.supports_chatgpt_subscription_plan(),
                "{provider:?} must stay API-key-only"
            );
        }
    }

    #[test]
    fn oauth_token_debug_redacts_both_secrets() {
        // Issue #599 R2a: refresh tokens must never reach logs. The Debug
        // impl renders byte counts; a regression that echoes a secret fails
        // this test instead of leaking at runtime.
        let token = EndpointOAuthToken {
            endpoint_id: uuid::Uuid::new_v4(),
            access_token: "access-secret-value".to_string(),
            refresh_token: "refresh-secret-value".to_string(),
            expires_at: None,
        };
        let rendered = format!("{token:?}");
        assert!(
            !rendered.contains("access-secret-value"),
            "access token must not leak into Debug: {rendered}"
        );
        assert!(
            !rendered.contains("refresh-secret-value"),
            "refresh token must not leak into Debug: {rendered}"
        );
        assert!(rendered.contains("[REDACTED;"));
        let staged = EndpointOAuthTokenSet {
            access_token: "access-secret-value".to_string(),
            refresh_token: "refresh-secret-value".to_string(),
            expires_at: None,
        };
        let staged_rendered = format!("{staged:?}");
        assert!(!staged_rendered.contains("access-secret-value"));
        assert!(!staged_rendered.contains("refresh-secret-value"));
    }

    #[test]
    fn provider_capabilities_are_scoped() {
        // Issue #562: the pre-flight thinking downgrade only applies to the
        // upstream confirmed to reject a tool-bearing thinking turn without a
        // reasoning echo (DeepSeek, issue #556 fingerprint). Every other
        // provider forwards the requested thinking unchanged.
        assert!(EndpointProvider::DeepSeek.requires_reasoning_echo());
        for provider in [
            EndpointProvider::Generic,
            EndpointProvider::Minimax,
            EndpointProvider::CommandCode,
            EndpointProvider::OpencodeGo,
            EndpointProvider::OpenRouter,
            EndpointProvider::Glm,
            EndpointProvider::OpenAi,
        ] {
            assert!(
                !provider.requires_reasoning_echo(),
                "{provider:?} must not be pre-flight downgraded"
            );
        }
    }

    #[test]
    fn service_tier_defaults_to_standard_and_parses_priority() {
        assert_eq!(MinimaxServiceTier::default(), MinimaxServiceTier::Standard);
        assert_eq!(
            MinimaxServiceTier::from_optional(None),
            MinimaxServiceTier::Standard
        );
        assert_eq!(
            MinimaxServiceTier::from_optional(Some("priority")),
            MinimaxServiceTier::Priority
        );
        assert_eq!(
            MinimaxServiceTier::from_optional(Some("standard")),
            MinimaxServiceTier::Standard
        );
        assert_eq!(
            MinimaxServiceTier::from_optional(Some("legacy-unknown")),
            MinimaxServiceTier::Standard
        );
        assert_eq!(MinimaxServiceTier::Priority.as_str(), "priority");
        // Legacy/omitted JSON values deserialize as standard.
        let create: EndpointCreate = serde_json::from_value(serde_json::json!({
            "scope": "admin",
            "name": "legacy",
            "provider": "minimax",
            "provider_region": "global",
            "base_url": "https://api.minimaxi.com",
            "native_api": "chat",
            "native_api_source": "manual",
            "api_key": "key",
            "api_keys": [],
            "key_lb_enabled": false,
            "enabled": true
        }))
        .expect("legacy endpoint create without service_tier");
        assert_eq!(create.service_tier, MinimaxServiceTier::Standard);
    }
}
