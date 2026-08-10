use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::{McpCredential, McpQuotaAccountSnapshot, McpQuotaGroup, QuotaUnit};

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct QuotaGroupRequest {
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

impl From<QuotaGroupRequest> for crate::db::McpQuotaGroupInput {
    fn from(value: QuotaGroupRequest) -> Self {
        Self {
            name: value.name,
            scope: value.scope,
            owner_user_id: value.owner_user_id,
            provider_kind: value.provider_kind,
            unit: value.unit,
            daily_limit: value.daily_limit,
            monthly_limit: value.monthly_limit,
            default_cost: value.default_cost,
            strict_mode: value.strict_mode,
            billing_period_start: value.billing_period_start,
            billing_period_end: value.billing_period_end,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CredentialQuotaBindingRequest {
    pub quota_group_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CredentialPageResponse {
    pub credentials: Vec<McpCredential>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QuotaGroupUsageResponse {
    pub group: McpQuotaGroup,
    pub day: Option<McpQuotaAccountSnapshot>,
    pub month: Option<McpQuotaAccountSnapshot>,
}
