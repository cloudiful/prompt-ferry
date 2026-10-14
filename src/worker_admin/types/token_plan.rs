use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::db::{EndpointProvider, EndpointRegion};

use super::{
    endpoints::{
        SubscriptionQuotaObservation, SubscriptionWindowAvailability, SubscriptionWindowUsage,
    },
    quota_status::TokenPlanQuotaStatus,
};

#[derive(Debug, Clone, Copy, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TokenPlanUsageQuery {
    #[serde(default)]
    #[param(default = false)]
    pub force: Option<bool>,
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
    /// Effective default-text quota state; raw provider detail remains unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_status: Option<TokenPlanQuotaStatus>,
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
    /// Additive canonical quota observations (issue #844 P1). `None` means
    /// legacy fallback (populated only by newer backends); `Some([])` is an
    /// explicit assessed empty (successful fetch with no rows); otherwise one
    /// row per provider slot/model/currency. Failed keys stay `None` so an
    /// unknown failure never falls through into available capacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<CanonicalQuotaWindow>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanModelUsage {
    pub model_name: String,
    pub interval: Option<TokenPlanWindowUsage>,
    pub weekly: Option<TokenPlanWindowUsage>,
    /// Issue #759 P1: canonical ChatGPT subscription windows with their real
    /// reported duration. Populated only for ChatGPT subscription endpoints;
    /// every other provider leaves it `None` and keeps the positional
    /// `interval`/`weekly` slots unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<SubscriptionWindowUsage>>,
    /// Observation freshness/error state for persisted ChatGPT quota snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<SubscriptionQuotaObservation>,
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
/// decimal strings, so the parser coerces both strings and numbers; a missing,
/// `null`, or unparseable amount is `null` (unknown) while a real zero stays
/// `0`. Every parseable currency entry is preserved with its identifier
/// normalized to upper case. A balance carries no quota window:
/// `is_available` alone drives routing weight.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeepSeekBalance {
    pub is_available: bool,
    /// Every parseable `balance_infos` entry, ordered by currency and then by
    /// amount, so a repeated currency stays complete and deterministic.
    pub balances: Vec<DeepSeekCurrencyBalance>,
}

/// One DeepSeek currency entry. `None` amounts are unknown — the provider
/// omitted the field or the parser could not coerce it — never a zero.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct DeepSeekCurrencyBalance {
    pub currency: String,
    pub total_balance: Option<f64>,
    pub granted_balance: Option<f64>,
    pub topped_up_balance: Option<f64>,
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

/// Stable kind for one canonical quota observation (issue #844 P1).
/// Time windows carry a usable percentage; `balance`/`unlimited` carry
/// amounts only and never a synthetic percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalQuotaWindowKind {
    FiveHour,
    Rolling,
    Weekly,
    Monthly,
    Balance,
    Unlimited,
    Custom,
}

/// One additive canonical quota observation (issue #844 P1). `source_id`
/// is the stable provider slot/model/currency identity (never a secret);
/// amounts preserve presence (`Some(0.0)` is a real zero, `None` is
/// absent/non-finite); `remaining_percent` is a valid finite reported or
/// derived share only (unknown never `0`/`100`); `balance`/`unlimited`
/// rows never carry a synthetic percentage.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, ToSchema)]
pub struct CanonicalQuotaWindow {
    pub source_id: String,
    pub kind: CanonicalQuotaWindowKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i64>,
    pub is_default_scope: bool,
    pub availability: SubscriptionWindowAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<DateTime<Utc>>,
}
