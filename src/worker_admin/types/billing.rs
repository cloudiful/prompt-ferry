use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BillingPriceSide {
    Cost,
    Sale,
}

impl BillingPriceSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cost => "cost",
            Self::Sale => "sale",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BillingPriceRuleRequest {
    pub price_side: BillingPriceSide,
    pub public_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub upstream_model: Option<String>,
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
    pub price_side: String,
    pub public_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub upstream_model: Option<String>,
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
    pub provider_cost: Option<String>,
    pub adjusted_amount: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingSummaryResponse {
    pub currency: String,
    pub request_count: i64,
    pub known_count: i64,
    pub unknown_count: i64,
    pub priced_count: i64,
    pub unpriced_count: i64,
    pub provider_cost: Option<String>,
    pub customer_amount: String,
    pub adjusted_amount: String,
    pub gross_margin: Option<String>,
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
    pub provider_cost: Option<String>,
    pub customer_amount: Option<String>,
    pub adjusted_amount: Option<String>,
    pub gross_margin: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingChargePageResponse {
    pub total: i64,
    pub charges: Vec<BillingChargeResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingChargeLineResponse {
    pub line_id: i64,
    pub price_side: String,
    pub meter: String,
    pub token_count: i64,
    pub unit_rate: String,
    pub amount: String,
    pub price_rule_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BillingAdjustmentResponse {
    pub adjustment_id: i64,
    pub amount: String,
    pub reason: String,
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingChargeDetailResponse {
    pub charge: BillingChargeResponse,
    pub lines: Vec<BillingChargeLineResponse>,
    pub adjustments: Vec<BillingAdjustmentResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BillingAdjustmentRequest {
    pub amount: String,
    pub reason: String,
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
