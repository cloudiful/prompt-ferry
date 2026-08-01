use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Debug, Deserialize, ToSchema)]
pub struct BillingPriceRuleRequest {
    pub public_model: String,
    pub input_rate: String,
    pub cache_read_rate: String,
    pub cache_write_rate: String,
    pub output_rate: String,
    pub effective_from: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BillingPriceRulePatch {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingPriceRuleResponse {
    pub price_rule_id: Uuid,
    pub public_model: String,
    pub input_rate: String,
    pub cache_read_rate: String,
    pub cache_write_rate: String,
    pub output_rate: String,
    pub currency: String,
    pub effective_from: DateTime<Utc>,
    pub effective_to: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingPriceRulePageResponse {
    pub rules: Vec<BillingPriceRuleResponse>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingPriceRulesQuery {
    pub first: Option<i64>,
    pub rows: Option<i64>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingChargesQuery {
    pub first: Option<i64>,
    pub rows: Option<i64>,
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub requested_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub usage_status: Option<String>,
    pub pricing_status: Option<String>,
    pub request_id: Option<Uuid>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingSummaryQuery {
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub requested_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub usage_status: Option<String>,
    pub pricing_status: Option<String>,
    pub request_id: Option<Uuid>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingBreakdownResponse {
    pub grouping_key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub customer_amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingSummaryResponse {
    pub currency: String,
    pub request_count: i64,
    pub known_count: i64,
    pub unknown_count: i64,
    pub priced_count: i64,
    pub unpriced_count: i64,
    pub customer_amount: String,
    pub by_client_key: Vec<BillingBreakdownResponse>,
    pub by_model: Vec<BillingBreakdownResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingChargeResponse {
    pub charge_id: i64,
    pub request_id: Uuid,
    pub user_id: Option<i64>,
    pub user_login_name: Option<String>,
    pub client_key_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub endpoint_key_id: Option<Uuid>,
    pub usage_status: String,
    pub pricing_status: String,
    pub currency: String,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub customer_amount: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingChargePageResponse {
    pub total: i64,
    pub charges: Vec<BillingChargeResponse>,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingChargeLineResponse {
    pub line_id: i64,
    pub meter: String,
    pub token_count: i64,
    pub unit_rate: String,
    pub amount: String,
    pub price_rule_id: Option<Uuid>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingChargeDetailResponse {
    pub charge: BillingChargeResponse,
    pub lines: Vec<BillingChargeLineResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BillingRepriceRequest {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingRepriceResponse {
    pub repriced: u64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillingExportQuery {
    pub kind: Option<String>,
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub requested_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub usage_status: Option<String>,
    pub pricing_status: Option<String>,
    pub request_id: Option<Uuid>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}
