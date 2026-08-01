use super::*;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::{self, BillingChargeFilter, BillingPriceRuleCreate, BillingPriceRuleUpdate};

pub(super) async fn list_billing_price_rules(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<BillingPriceRulesQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let first = query.first.unwrap_or(0).clamp(0, 10_000);
    let rows = query.rows.unwrap_or(100).clamp(1, 1_000);
    match db::list_price_rules(&state.pool, rows, first).await {
        Ok(rules) => {
            let total = match db::count_price_rules(&state.pool).await {
                Ok(total) => total,
                Err(err) => return internal(&state, err),
            };
            Json(BillingPriceRulePageResponse {
                rules: rules.into_iter().map(price_rule_response).collect(),
                total,
                first,
                rows,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_billing_price_rule(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<BillingPriceRuleRequest>,
) -> Response {
    let user = match ensure_admin(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let input = match billing_price_rule_input(body, user.user_id) {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::create_price_rule(&state.pool, input).await {
        Ok(rule) => Json(price_rule_response(rule)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn patch_billing_price_rule(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(price_rule_id): Path<Uuid>,
    Json(body): Json<BillingPriceRulePatch>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::update_price_rule_status(&state.pool, price_rule_id, body.enabled).await {
        Ok(Some(rule)) => Json(price_rule_response(rule)).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "price rule not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_billing_price_rule(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(price_rule_id): Path<Uuid>,
    Json(body): Json<BillingPriceRuleRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let input = match billing_price_rule_update_input(body) {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::update_price_rule(&state.pool, price_rule_id, input).await {
        Ok(Some(rule)) => Json(price_rule_response(rule)).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "price rule not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_billing_price_rule(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(price_rule_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::delete_price_rule(&state.pool, price_rule_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "price rule not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn billing_summary(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<BillingSummaryQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let filter = BillingChargeFilter {
        user_id: if user.is_admin {
            query.user_id
        } else {
            Some(user.user_id)
        },
        client_key_id: query.client_key_id,
        requested_model: query.requested_model,
        endpoint_id: query.endpoint_id,
        usage_status: query.usage_status,
        pricing_status: query.pricing_status,
        request_id: query.request_id,
        start_at: query.start_at,
        end_at: query.end_at,
    };
    match db::billing_summary(&state.pool, &filter).await {
        Ok(summary) => Json(summary_response(summary)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn list_billing_charges(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<BillingChargesQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let filter = BillingChargeFilter {
        user_id: if user.is_admin {
            query.user_id
        } else {
            Some(user.user_id)
        },
        client_key_id: query.client_key_id,
        requested_model: query.requested_model,
        endpoint_id: query.endpoint_id,
        usage_status: query.usage_status,
        pricing_status: query.pricing_status,
        request_id: query.request_id,
        start_at: query.start_at,
        end_at: query.end_at,
    };
    let first = query.first.unwrap_or(0).clamp(0, 1_000_000);
    let rows = query.rows.unwrap_or(50).clamp(1, 500);
    match db::list_charges(&state.pool, &filter, first, rows).await {
        Ok((total, charges)) => Json(BillingChargePageResponse {
            total,
            charges: charges
                .into_iter()
                .map(|charge| charge_response(charge, user.is_admin))
                .collect(),
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn billing_charge_detail(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(charge_id): Path<i64>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let detail = match db::get_charge(&state.pool, charge_id).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "charge not found"),
        Err(err) => return internal(&state, err),
    };
    if !user.is_admin && detail.charge.user_id != Some(user.user_id) {
        return error(StatusCode::NOT_FOUND, "not_found", "charge not found");
    }
    let lines = detail.lines.into_iter().map(line_response).collect();
    Json(BillingChargeDetailResponse {
        charge: charge_response(detail.charge, user.is_admin),
        lines,
    })
    .into_response()
}

pub(super) async fn reprice_billing(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<BillingRepriceRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::reprice_unpriced_charges(&state.pool, body.limit.unwrap_or(1_000)).await {
        Ok(repriced) => Json(BillingRepriceResponse { repriced }).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn export_billing(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<BillingExportQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let filter = BillingChargeFilter {
        user_id: if user.is_admin {
            query.user_id
        } else {
            Some(user.user_id)
        },
        client_key_id: query.client_key_id,
        requested_model: query.requested_model,
        endpoint_id: query.endpoint_id,
        usage_status: query.usage_status,
        pricing_status: query.pricing_status,
        request_id: query.request_id,
        start_at: query.start_at,
        end_at: query.end_at,
    };
    let kind = query.kind.as_deref().unwrap_or("details");
    let csv = if matches!(kind, "monthly" | "summary") {
        match db::list_monthly_export(&state.pool, &filter).await {
            Ok(rows) => monthly_csv(rows),
            Err(err) => return internal(&state, err),
        }
    } else {
        let rows = match db::list_charge_export(&state.pool, &filter).await {
            Ok(rows) => rows,
            Err(err) => return internal(&state, err),
        };
        detail_csv(rows, user.is_admin)
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=prompt-ferry-billing-{kind}.csv"),
        )
        .body(csv.into())
        .unwrap_or_else(|err| internal(&state, anyhow::anyhow!(err)))
}

fn billing_price_rule_input(
    body: BillingPriceRuleRequest,
    created_by_user_id: i64,
) -> Result<BillingPriceRuleCreate, Response> {
    let BillingPriceRuleUpdate {
        public_model,
        input_rate,
        cache_read_rate,
        cache_write_rate,
        output_rate,
        effective_from,
    } = billing_price_rule_update_input(body)?;
    Ok(BillingPriceRuleCreate {
        public_model,
        input_rate,
        cache_read_rate,
        cache_write_rate,
        output_rate,
        effective_from,
        created_by_user_id,
    })
}

fn billing_price_rule_update_input(
    body: BillingPriceRuleRequest,
) -> Result<BillingPriceRuleUpdate, Response> {
    let public_model = body.public_model.trim().to_string();
    if public_model.is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_public_model",
            "public_model must not be empty",
        ));
    }
    let input_rate = parse_decimal(&body.input_rate, "input_rate")?;
    let cache_read_rate = parse_decimal(&body.cache_read_rate, "cache_read_rate")?;
    let cache_write_rate = parse_decimal(&body.cache_write_rate, "cache_write_rate")?;
    let output_rate = parse_decimal(&body.output_rate, "output_rate")?;
    if [input_rate, cache_read_rate, cache_write_rate, output_rate]
        .into_iter()
        .any(|value| value.is_sign_negative())
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_rate",
            "rates must be non-negative",
        ));
    }
    Ok(BillingPriceRuleUpdate {
        public_model,
        input_rate,
        cache_read_rate,
        cache_write_rate,
        output_rate,
        effective_from: body.effective_from,
    })
}

fn parse_decimal(value: &str, field: &str) -> Result<Decimal, Response> {
    Decimal::from_str(value.trim()).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "invalid_decimal",
            &format!("{field} must be a decimal string"),
        )
    })
}

fn decimal_string(value: Decimal) -> String {
    value.normalize().to_string()
}

fn price_rule_response(rule: db::BillingPriceRuleRow) -> BillingPriceRuleResponse {
    BillingPriceRuleResponse {
        price_rule_id: rule.price_rule_id,
        public_model: rule.public_model,
        input_rate: decimal_string(rule.input_rate),
        cache_read_rate: decimal_string(rule.cache_read_rate),
        cache_write_rate: decimal_string(rule.cache_write_rate),
        output_rate: decimal_string(rule.output_rate),
        currency: rule.currency,
        effective_from: rule.effective_from,
        effective_to: rule.effective_to,
        enabled: rule.enabled,
        created_by_user_id: rule.created_by_user_id,
        created_at: rule.created_at,
        updated_at: rule.updated_at,
    }
}

fn summary_response(summary: db::BillingSummary) -> BillingSummaryResponse {
    BillingSummaryResponse {
        currency: "CNY".to_string(),
        request_count: summary.summary.request_count,
        known_count: summary.summary.known_count,
        unknown_count: summary.summary.unknown_count,
        priced_count: summary.summary.priced_count,
        unpriced_count: summary.summary.unpriced_count,
        customer_amount: decimal_string(summary.summary.customer_amount),
        by_client_key: summary
            .by_client_key
            .into_iter()
            .map(breakdown_response)
            .collect(),
        by_model: summary
            .by_model
            .into_iter()
            .map(breakdown_response)
            .collect(),
    }
}

fn breakdown_response(row: db::BillingBreakdownRow) -> BillingBreakdownResponse {
    BillingBreakdownResponse {
        grouping_key: row.grouping_key,
        request_count: row.request_count,
        input_tokens: row.input_tokens,
        cache_read_tokens: row.cache_read_tokens,
        cache_write_tokens: row.cache_write_tokens,
        output_tokens: row.output_tokens,
        customer_amount: decimal_string(row.customer_amount),
    }
}

fn charge_response(row: db::BillingChargeRow, is_admin: bool) -> BillingChargeResponse {
    BillingChargeResponse {
        charge_id: row.charge_id,
        request_id: row.request_id,
        user_id: is_admin.then_some(row.user_id).flatten(),
        user_login_name: is_admin.then_some(row.user_login_name).flatten(),
        client_key_id: row.client_key_id,
        client_key_label: row.client_key_label,
        requested_model: row.requested_model,
        upstream_model: is_admin.then_some(row.upstream_model).flatten(),
        endpoint_id: is_admin.then_some(row.endpoint_id).flatten(),
        endpoint_name: is_admin.then_some(row.endpoint_name).flatten(),
        endpoint_key_id: is_admin.then_some(row.endpoint_key_id).flatten(),
        usage_status: row.usage_status,
        pricing_status: row.pricing_status,
        currency: row.currency,
        input_tokens: row.input_tokens,
        cache_read_tokens: row.cache_read_tokens,
        cache_write_tokens: row.cache_write_tokens,
        output_tokens: row.output_tokens,
        customer_amount: row.customer_amount.map(decimal_string),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn line_response(line: db::BillingChargeLineRow) -> BillingChargeLineResponse {
    BillingChargeLineResponse {
        line_id: line.line_id,
        meter: line.meter,
        token_count: line.token_count,
        unit_rate: decimal_string(line.unit_rate),
        amount: decimal_string(line.amount),
        price_rule_id: line.price_rule_id,
    }
}

fn monthly_csv(rows: Vec<db::BillingMonthlyExportRow>) -> String {
    let mut output = String::from(
        "month,currency,request_count,known_count,unknown_count,priced_count,unpriced_count,customer_amount",
    );
    output.push('\n');
    for row in rows {
        let fields = vec![
            row.month.format("%Y-%m").to_string(),
            "CNY".to_string(),
            row.request_count.to_string(),
            row.known_count.to_string(),
            row.unknown_count.to_string(),
            row.priced_count.to_string(),
            row.unpriced_count.to_string(),
            decimal_string(row.customer_amount),
        ];
        let refs = fields.iter().map(String::as_str).collect::<Vec<_>>();
        output.push_str(&csv_row(&refs));
    }
    output
}

fn detail_csv(rows: Vec<db::BillingExportRow>, is_admin: bool) -> String {
    let mut output = String::from(
        "charge_id,request_id,user,client_key,requested_model,usage_status,pricing_status,input_tokens,cache_read_tokens,cache_write_tokens,output_tokens,customer_amount",
    );
    if is_admin {
        output.push_str(",upstream_model,endpoint");
    }
    output.push_str(",created_at\n");
    for row in rows {
        let mut fields = vec![
            row.charge_id.to_string(),
            row.request_id.to_string(),
            row.user_login_name.unwrap_or_default(),
            row.client_key_label.unwrap_or_default(),
            row.requested_model.unwrap_or_default(),
            row.usage_status,
            row.pricing_status,
            row.input_tokens.to_string(),
            row.cache_read_tokens.to_string(),
            row.cache_write_tokens.to_string(),
            row.output_tokens.to_string(),
            row.customer_amount.map(decimal_string).unwrap_or_default(),
        ];
        if is_admin {
            fields.push(row.upstream_model.unwrap_or_default());
            fields.push(row.endpoint_name.unwrap_or_default());
        }
        fields.push(row.created_at.to_rfc3339());
        let refs = fields.iter().map(String::as_str).collect::<Vec<_>>();
        output.push_str(&csv_row(&refs));
    }
    output
}

fn csv_row(fields: &[&str]) -> String {
    let mut row = fields
        .iter()
        .map(|field| format!("\"{}\"", field.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    row.push('\n');
    row
}
