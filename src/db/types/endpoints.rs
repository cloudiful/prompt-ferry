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
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "minimax" => Self::Minimax,
            _ => Self::Generic,
        }
    }

    pub fn from_optional(value: Option<&str>) -> Self {
        match value {
            Some("minimax") => Self::Minimax,
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
