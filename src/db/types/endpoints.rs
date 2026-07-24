use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::config::{NativeApi, NativeApiSource};

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
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderEndpoint {
    pub endpoint_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub key_lb_enabled: bool,
    pub enabled: bool,
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
            base_url: value.base_url,
            native_api: value.native_api,
            native_api_source: value.native_api_source,
            daily_max_requests: value.daily_max_requests,
            monthly_max_requests: value.monthly_max_requests,
            api_key: value.api_key,
            key_lb_enabled: value.key_lb_enabled,
            enabled: value.enabled,
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
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EndpointPage {
    pub total: i64,
    pub endpoints: Vec<ProviderEndpoint>,
    pub first: i64,
    pub rows: i64,
}
