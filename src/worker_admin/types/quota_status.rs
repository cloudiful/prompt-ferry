use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenPlanQuotaState {
    Available,
    Exhausted,
    Unknown,
    /// Blocked because the quota or balance is confirmed spent.
    Cooling,
    /// Blocked by throttling; the account's capacity is not reduced.
    RateLimited,
    /// Blocked because the upstream explicitly reported it is unavailable.
    UpstreamUnavailable,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TokenPlanQuotaStatus {
    pub state: TokenPlanQuotaState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub refresh_failed: bool,
    #[schema(example = "text_inference")]
    pub scope: String,
}
