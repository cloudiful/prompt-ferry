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
            _ => Self::Generic,
        }
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
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    #[serde(skip_serializing)]
    pub api_key: String,
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
    #[serde(default)]
    pub service_tier: MinimaxServiceTier,
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    #[serde(skip_serializing)]
    pub api_key: String,
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
        Self {
            endpoint_id: value.endpoint_id,
            scope: value.scope,
            owner_user_id: value.owner_user_id,
            name: value.name,
            provider: EndpointProvider::from_str(&value.provider),
            provider_region: EndpointRegion::from_str(value.provider_region.as_deref()),
            service_tier: MinimaxServiceTier::from_optional(value.service_tier.as_deref()),
            base_url: value.base_url,
            native_api: value.native_api,
            native_api_source: value.native_api_source,
            daily_max_requests: value.daily_max_requests,
            monthly_max_requests: value.monthly_max_requests,
            api_key: value.api_key,
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
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub api_key: String,
    pub api_keys: Vec<EndpointApiKeyCreate>,
    pub key_lb_enabled: bool,
    pub enabled: bool,
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
