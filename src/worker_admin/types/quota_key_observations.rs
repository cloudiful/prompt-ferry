use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::token_plan::CanonicalQuotaWindow;

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
