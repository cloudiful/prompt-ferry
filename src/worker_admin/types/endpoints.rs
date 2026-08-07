use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{config::NativeApi, db};

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct EndpointApiKeyRequest {
    pub key_label: String,
    pub api_key: String,
    pub enabled: Option<bool>,
    #[serde(default)]
    /// Stable key identity; when omitted or null the key is matched by key_label on update
    pub key_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EndpointRequest {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub api_keys: Vec<EndpointApiKeyRequest>,
    #[serde(default)]
    pub key_lb_enabled: bool,
    pub protocol_mode: EndpointProtocolMode,
    pub native_api_override: Option<NativeApi>,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EndpointProtocolMode {
    Auto,
    Manual,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EndpointSettingRequest {
    pub endpoint_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConversationEndpointOverrideRequest {
    pub endpoint_id: Uuid,
    pub endpoint_key_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionAffinityState {
    Unbound,
    Active,
    StaleEndpoint,
    StaleKey,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionAffinityStatus {
    pub state: SessionAffinityState,
    pub rule_id: Option<Uuid>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub key_id: Option<Uuid>,
    pub key_label: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionAffinityResetResponse {
    pub cleared: bool,
    pub cleared_count: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionRouteOptionsResponse {
    pub conversation_id: Uuid,
    pub current_endpoint_id: Option<Uuid>,
    pub current_endpoint_key_id: Option<Uuid>,
    pub current_endpoint_key_label: Option<String>,
    pub override_endpoint_id: Option<Uuid>,
    pub override_endpoint_key_id: Option<Uuid>,
    pub override_endpoint_key_label: Option<String>,
    pub options: Vec<db::SessionRouteOption>,
    pub affinity: SessionAffinityStatus,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TablePageQuery {
    pub first: Option<i64>,
    pub rows: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EndpointPageResponse {
    pub total: i64,
    pub endpoints: Vec<db::ProviderEndpoint>,
    pub first: i64,
    pub rows: i64,
}

impl From<db::EndpointPage> for EndpointPageResponse {
    fn from(value: db::EndpointPage) -> Self {
        Self {
            total: value.total,
            endpoints: value.endpoints,
            first: value.first,
            rows: value.rows,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EndpointTestResponse {
    pub ok: bool,
    pub status: Option<u16>,
    #[schema(value_type = u64)]
    pub duration_ms: u128,
    pub model_count: Option<usize>,
    pub native_api: Option<String>,
    pub native_api_source: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RealtimeClientSecretRequest {
    pub session: serde_json::Value,
    pub expires_after: Option<RealtimeClientSecretExpiresAfter>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RealtimeClientSecretExpiresAfter {
    pub anchor: Option<String>,
    pub seconds: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RealtimeClientSecretResponse {
    pub value: String,
    pub expires_at: u64,
    pub session: serde_json::Value,
}
