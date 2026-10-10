use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::SubscriptionWindowUsage;

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuotaSnapshotHistoryQuery {
    #[serde(default)]
    #[param(default = 50, minimum = 1, maximum = 200)]
    pub limit: Option<u16>,
    #[serde(default)]
    pub before_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaSnapshotHistoryItem {
    pub snapshot_id: String,
    pub observed_at: DateTime<Utc>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Vec<SubscriptionWindowUsage>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaSnapshotHistoryResponse {
    pub items: Vec<QuotaSnapshotHistoryItem>,
    pub next_cursor: Option<String>,
    pub retention_days: u16,
}
