use crate::worker_admin_types::{
    BillingAdjustmentRequest, BillingAdjustmentResponse, BillingChargeDetailResponse,
    BillingChargePageResponse, BillingChargesQuery, BillingExportQuery,
    BillingPriceRulePageResponse, BillingPriceRulePatch, BillingPriceRuleRequest,
    BillingPriceRuleResponse, BillingPriceRulesQuery, BillingRepriceRequest,
    BillingRepriceResponse, BillingSummaryQuery, BillingSummaryResponse,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        list_billing_price_rules,
        create_billing_price_rule,
        patch_billing_price_rule,
        billing_summary,
        list_billing_charges,
        billing_charge_detail,
        add_billing_adjustment,
        reprice_billing,
        export_billing
    ),
    components(
        schemas(
            BillingAdjustmentRequest,
            BillingAdjustmentResponse,
            BillingChargeDetailResponse,
            BillingChargePageResponse,
            BillingPriceRulePatch,
            BillingPriceRulePageResponse,
            BillingPriceRuleRequest,
            BillingPriceRuleResponse,
            BillingRepriceRequest,
            BillingRepriceResponse,
            BillingSummaryResponse
        )
    ),
    tags((name = "billing", description = "Usage billing and pricing"))
)]
pub(super) struct BillingApiDoc;

#[utoipa::path(
    get,
    path = "/api/v1/admin/billing/price-rules",
    params(BillingPriceRulesQuery),
    responses((status = 200, body = BillingPriceRulePageResponse, description = "Billing price rules")),
    tag = "billing"
)]
pub(super) fn list_billing_price_rules() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/billing/price-rules",
    request_body = BillingPriceRuleRequest,
    responses((status = 200, body = BillingPriceRuleResponse, description = "Created billing price rule")),
    tag = "billing"
)]
pub(super) fn create_billing_price_rule() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/billing/price-rules/{price_rule_id}",
    params(("price_rule_id" = uuid::Uuid, Path, description = "Price rule ID")),
    request_body = BillingPriceRulePatch,
    responses((status = 200, body = BillingPriceRuleResponse, description = "Updated price rule status")),
    tag = "billing"
)]
pub(super) fn patch_billing_price_rule() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/billing/summary",
    params(BillingSummaryQuery),
    responses((status = 200, body = BillingSummaryResponse, description = "Billing summary")),
    tag = "billing"
)]
pub(super) fn billing_summary() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/billing/charges",
    params(BillingChargesQuery),
    responses((status = 200, body = BillingChargePageResponse, description = "Billing charges")),
    tag = "billing"
)]
pub(super) fn list_billing_charges() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/billing/charges/{charge_id}",
    params(("charge_id" = i64, Path, description = "Charge ID")),
    responses((status = 200, body = BillingChargeDetailResponse, description = "Billing charge detail")),
    tag = "billing"
)]
pub(super) fn billing_charge_detail() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/billing/charges/{charge_id}/adjustments",
    params(("charge_id" = i64, Path, description = "Charge ID")),
    request_body = BillingAdjustmentRequest,
    responses((status = 200, body = BillingAdjustmentResponse, description = "Created billing adjustment")),
    tag = "billing"
)]
pub(super) fn add_billing_adjustment() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/billing/reprice-unpriced",
    request_body = BillingRepriceRequest,
    responses((status = 200, body = BillingRepriceResponse, description = "Repricing result")),
    tag = "billing"
)]
pub(super) fn reprice_billing() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/billing/export",
    params(BillingExportQuery),
    responses((status = 200, content_type = "text/csv", body = String, description = "Billing CSV export")),
    tag = "billing"
)]
pub(super) fn export_billing() {}
