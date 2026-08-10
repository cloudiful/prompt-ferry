use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum QuotaUnit {
    Requests,
    Credits,
}

impl QuotaUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Credits => "credits",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum QuotaPeriodKind {
    Day,
    Month,
}

impl QuotaPeriodKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct QuotaPeriod {
    pub kind: QuotaPeriodKind,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct McpQuotaGroup {
    pub group_id: uuid::Uuid,
    pub name: String,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub provider_kind: Option<String>,
    pub unit: String,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: f64,
    pub strict_mode: bool,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpQuotaGroupInput {
    pub name: String,
    pub scope: Option<String>,
    pub owner_user_id: Option<i64>,
    pub provider_kind: Option<String>,
    pub unit: Option<QuotaUnit>,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: Option<f64>,
    pub strict_mode: Option<bool>,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct McpCredential {
    pub credential_id: uuid::Uuid,
    pub server_id: uuid::Uuid,
    pub credential_label: String,
    #[serde(skip_serializing)]
    pub secret: String,
    pub position: i32,
    pub enabled: bool,
    pub quota_group_id: Option<uuid::Uuid>,
    pub provider_kind: Option<String>,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: f64,
    pub strict_mode: bool,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
    pub provider_remaining: Option<f64>,
    pub provider_synced_at: Option<DateTime<Utc>>,
    pub provider_reset_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl McpCredential {
    pub fn is_exhausted(&self) -> bool {
        self.provider_remaining
            .is_some_and(|remaining| remaining <= 0.0)
    }

    pub fn is_in_cooldown(&self, now: DateTime<Utc>) -> bool {
        self.cooldown_until.is_some_and(|until| until > now)
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct McpQuotaAccountRow {
    pub account_id: i64,
    pub group_id: uuid::Uuid,
    pub period_kind: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub used_units: f64,
    pub reserved_units: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpQuotaAccountSnapshot {
    pub account_id: i64,
    pub period: QuotaPeriod,
    pub used_units: f64,
    pub reserved_units: f64,
}

#[derive(Debug, Clone)]
pub struct QuotaReservation {
    pub reservation_id: i64,
    pub account_id: i64,
    pub credential_id: uuid::Uuid,
    pub request_id: uuid::Uuid,
    pub units: f64,
}

#[derive(Debug, Clone)]
pub struct QuotaGrant {
    pub credential: McpCredential,
    pub reservation: QuotaReservation,
    /// Additional account rows updated for the day dimension, when present.
    pub day_account: Option<McpQuotaAccountSnapshot>,
    pub month_account: Option<McpQuotaAccountSnapshot>,
}
