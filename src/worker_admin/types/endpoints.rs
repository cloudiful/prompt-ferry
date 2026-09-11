use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    config::NativeApi,
    db::{self, EndpointProvider, EndpointRegion, MinimaxServiceTier},
};

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
    #[serde(default)]
    pub provider: EndpointProvider,
    #[serde(default)]
    pub provider_region: Option<EndpointRegion>,
    #[serde(default)]
    pub service_tier: MinimaxServiceTier,
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
    #[serde(default)]
    pub mcp_enabled: Option<bool>,
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

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanUsageResponse {
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    pub keys: Vec<TokenPlanKeyUsage>,
    /// AI tokens recorded for this endpoint since the start of the UTC day,
    /// aggregated locally from `request_records`. Populated for balance
    /// providers without a provider-reported spend figure (OpenRouter
    /// fallback, DeepSeek) so the badge can pair the balance with a
    /// "today usage" pill. `None` when not computed/applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_today_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanKeyUsage {
    pub key_id: Uuid,
    pub key_label: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub model_remains: Vec<TokenPlanModelUsage>,
    /// CommandCode credit balances (monthly/purchased/free). None for MiniMax.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balances: Option<CommandCodeBalances>,
    /// CommandCode 5-hour USD window. None when missing (e.g. PAYG) — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<CommandCodeWindowUsage>,
    /// CommandCode weekly USD window. None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly: Option<CommandCodeWindowUsage>,
    /// OpencodeGo rolling window usage. None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencodego_rolling: Option<OpencodeGoWindowUsage>,
    /// OpencodeGo weekly window usage. None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencodego_weekly: Option<OpencodeGoWindowUsage>,
    /// OpencodeGo monthly window usage. None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencodego_monthly: Option<OpencodeGoWindowUsage>,
    /// OpenRouter credit balance (`GET /api/v1/key` limit fields plus
    /// `GET /api/v1/credits` totals). None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter_balance: Option<OpenRouterBalance>,
    /// OpenRouter spend (`GET /api/v1/key` usage fields). None when missing
    /// — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter_spend: Option<OpenRouterSpend>,
    /// GLM 5-hour token/credit window. None when missing — degraded or the
    /// configured key is not on a Coding Plan that exposes that window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glm_five_hour: Option<GlmWindowUsage>,
    /// GLM weekly token/credit window. None when missing — degraded or the
    /// configured key is not on a Coding Plan that exposes that window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glm_weekly: Option<GlmWindowUsage>,
    /// DeepSeek balance (`GET /user/balance`). `is_available=false` means the
    /// account cannot spend: the key is weighted 0 and excluded from routing.
    /// None when missing — degraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deepseek_balance: Option<DeepSeekBalance>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanModelUsage {
    pub model_name: String,
    pub interval: Option<TokenPlanWindowUsage>,
    pub weekly: Option<TokenPlanWindowUsage>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanWindowUsage {
    pub status: Option<i32>,
    pub remaining_percent: Option<f64>,
    pub total_count: Option<i64>,
    pub usage_count: Option<i64>,
    pub boost_permille: Option<i64>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub remains_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CommandCodeBalances {
    pub monthly_credits: f64,
    pub purchased_credits: f64,
    pub free_credits: f64,
    pub remaining_credits: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CommandCodeWindowUsage {
    pub used: f64,
    pub cap: f64,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub reset_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OpencodeGoWindowUsage {
    pub status: Option<i32>,
    pub percent: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OpenRouterBalance {
    pub limit: Option<f64>,
    pub limit_remaining: Option<f64>,
    pub limit_reset: Option<String>,
    pub is_free_tier: bool,
    pub total_credits: Option<f64>,
    pub total_usage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OpenRouterSpend {
    pub usage: f64,
    pub daily: f64,
    pub weekly: f64,
    pub monthly: f64,
}

/// DeepSeek account balance (`GET /user/balance`). The API reports amounts as
/// decimal strings, so the parser coerces both strings and numbers. A balance
/// carries no quota window: `is_available` alone drives routing weight.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeepSeekBalance {
    pub is_available: bool,
    pub currency: String,
    pub total_balance: f64,
    pub granted_balance: f64,
    pub topped_up_balance: f64,
}

/// One GLM quota window (`TOKENS_LIMIT` or `CREDIT_LIMIT` row in the
/// `/api/monitor/usage/quota/limit` response). `percentage` is the used
/// share 0..=100 as reported by the API; cache weighting derives
/// `100 - percentage` to keep the schema symmetric with the other
/// providers. `next_reset_at` is normalized to `DateTime<Utc>`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GlmWindowUsage {
    pub limit: f64,
    pub current_value: f64,
    pub remaining: f64,
    /// Used share 0..=100 (matches Zhipu's `percentage` field). Derived
    /// from `currentValue / limit` when the API omits the field, and
    /// clamped to the closed interval.
    pub percentage: Option<f64>,
    pub next_reset_at: Option<DateTime<Utc>>,
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
