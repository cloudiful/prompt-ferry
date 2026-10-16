use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::token_plan::{CanonicalQuotaWindow, CanonicalQuotaWindowKind};

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuotaKeyObservationHistoryQuery {
    #[serde(default)]
    #[param(value_type = String, format = "uuid")]
    pub key_id: Option<String>,
    #[serde(default)]
    #[param(default = 50, minimum = 1, maximum = 200)]
    pub limit: Option<u16>,
    #[serde(default)]
    pub before_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaKeyObservationHistoryItem {
    pub observation_id: String,
    pub key_id: Uuid,
    pub key_label: String,
    pub provider: String,
    pub observed_at: DateTime<Utc>,
    pub ok: bool,
    pub status: Option<String>,
    pub error_code: Option<String>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Vec<CanonicalQuotaWindow>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaKeyObservationHistoryResponse {
    pub items: Vec<QuotaKeyObservationHistoryItem>,
    pub next_cursor: Option<String>,
    pub retention_days: u16,
}

/// Fixed-window query for the bounded quota observation trend. There is no
/// client-chosen interval, limit, or absolute date.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuotaKeyObservationTrendQuery {
    #[serde(default)]
    #[param(value_type = String, format = "uuid")]
    pub key_id: Option<String>,
    #[serde(default)]
    #[param(default = "24h")]
    pub range: Option<String>,
}

/// The numeric meaning of one trend series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuotaKeyObservationTrendMetric {
    Amount,
    RemainingPercent,
}

/// One fixed-grid point; `value`/`observed_at` are `null` for a missing bucket,
/// a failed snapshot, or an unavailable window.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaKeyObservationTrendPoint {
    pub at: DateTime<Utc>,
    pub observed_at: Option<DateTime<Utc>>,
    pub value: Option<f64>,
}

/// One identity-separated series; points are ascending and exactly
/// `bucket_count` long.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaKeyObservationTrendSeries {
    pub key_id: Uuid,
    pub key_label: String,
    pub source_id: String,
    pub kind: CanonicalQuotaWindowKind,
    pub unit: String,
    pub metric: QuotaKeyObservationTrendMetric,
    pub points: Vec<QuotaKeyObservationTrendPoint>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaKeyObservationTrendResponse {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub range: String,
    pub bucket_seconds: i64,
    pub bucket_count: u32,
    pub keys_truncated: bool,
    pub series_truncated: bool,
    pub sampled_observations: u32,
    pub failed_observations: u32,
    pub series: Vec<QuotaKeyObservationTrendSeries>,
}
