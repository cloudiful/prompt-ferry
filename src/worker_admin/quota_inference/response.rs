use super::estimate::Capacity;
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct Estimate {
    pub capacity_millions: Option<f64>,
    pub lower_millions: Option<f64>,
    pub upper_millions: Option<f64>,
    pub reason: Option<String>,
}
impl Estimate {
    pub(super) fn available(capacity: Capacity, divisor: f64) -> Self {
        Self {
            capacity_millions: Some(capacity.median / divisor),
            lower_millions: Some(capacity.lower / divisor),
            upper_millions: Some(capacity.upper / divisor),
            reason: None,
        }
    }
    pub fn unavailable(reason: &str) -> Self {
        Self {
            capacity_millions: None,
            lower_millions: None,
            upper_millions: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct ModelRow {
    pub model: String,
    pub mixed_tokens: i64,
    pub fresh_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub cache_hit: Option<f64>,
    pub output_share: Option<f64>,
    pub actual: Estimate,
    pub observed_near_97: Estimate,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct InferenceResponse {
    pub method_version: &'static str,
    pub status: &'static str,
    pub as_of: Option<DateTime<Utc>>,
    pub latest_snapshot_at: Option<DateTime<Utc>>,
    pub calculated_at: DateTime<Utc>,
    pub span_start: Option<DateTime<Utc>>,
    pub consumed_pp: f64,
    pub usable_blocks: usize,
    pub account: Estimate,
    pub models: Vec<ModelRow>,
    pub models_truncated: bool,
    pub unknown_tokens: i64,
    pub overflow_tokens: i64,
    pub omitted_rows: i64,
    pub observation_provenance: &'static str,
    pub account_history_assumed_stable: bool,
    pub current_refresh_failed: bool,
    pub account_identity_known: bool,
    pub cached: bool,
}
