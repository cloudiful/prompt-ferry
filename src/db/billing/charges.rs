use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{
    BillingBreakdownRow, BillingChargeFilter, BillingChargeLineRow, BillingChargeRow,
    BillingExportRow, BillingMeter, BillingMonthlyExportRow, BillingPriceRuleRow,
    BillingSummaryRow, NormalizedBillingUsage, RequestRecordCategory, RequestRecordCreate,
};

use super::prices::{match_cost_price_rule, match_sale_price_rule};

const TOKENS_PER_MILLION: i64 = 1_000_000;

#[derive(Debug, Clone)]
pub struct BillingSummary {
    pub summary: BillingSummaryRow,
    pub by_client_key: Vec<BillingBreakdownRow>,
    pub by_model: Vec<BillingBreakdownRow>,
}

#[derive(Debug, Clone)]
pub struct BillingChargeDetail {
    pub charge: BillingChargeRow,
    pub lines: Vec<BillingChargeLineRow>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct UnpricedChargeRow {
    charge_id: i64,
    requested_model: Option<String>,
    upstream_model: Option<String>,
    endpoint_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
}

pub async fn record_usage_charge(
    pool: &PgPool,
    event_id: i64,
    input: &RequestRecordCreate,
) -> Result<()> {
    if input.request_category != RequestRecordCategory::Ai {
        return Ok(());
    }

    let usage = normalized_usage(input);
    let usage_status = if usage.is_some() { "known" } else { "unknown" };
    let requested_model = input.requested_model.as_deref().or(input.model.as_deref());
    let upstream_model = input.upstream_model.as_deref().or(input.model.as_deref());
    let mut tx = pool.begin().await?;
    let at = sqlx::query_file_scalar!("src/sql/billing/charge_pricing_time.sql", event_id,)
        .fetch_one(&mut *tx)
        .await?;
    let sale_rule = match requested_model {
        Some(model) => match_sale_price_rule(&mut *tx, model, at)
            .await
            .with_context(|| {
                format!(
                    "billing price lookup failed: event_id={event_id} charge_id=<pending> \
                     price_side=sale public_model={model} endpoint_id={} upstream_model={} billing_at={at}",
                    input
                        .endpoint_id
                        .map_or_else(|| "<none>".to_string(), |id| id.to_string()),
                    upstream_model.unwrap_or("<none>"),
                )
            })?,
        None => None,
    };
    let cost_rule = match (input.endpoint_id, upstream_model) {
        (Some(endpoint_id), Some(model)) => match_cost_price_rule(&mut *tx, endpoint_id, model, at)
            .await
            .with_context(|| {
                format!(
                    "billing price lookup failed: event_id={event_id} charge_id=<pending> \
                         price_side=cost public_model={} endpoint_id={endpoint_id} \
                         upstream_model={model} billing_at={at}",
                    requested_model.unwrap_or("<none>"),
                )
            })?,
        _ => None,
    };
    let priced = usage.is_some() && sale_rule.is_some() && cost_rule.is_some();
    let pricing_status = if priced { "priced" } else { "unpriced" };
    let (provider_cost, customer_amount) = usage
        .map(|usage| {
            (
                cost_rule.as_ref().map(|rule| amount_for_usage(rule, usage)),
                sale_rule.as_ref().map(|rule| amount_for_usage(rule, usage)),
            )
        })
        .unwrap_or((None, None));
    let charge_id = sqlx::query_file_scalar!(
        "src/sql/billing/upsert_charge.sql",
        event_id,
        input.request_id,
        input.user_id,
        input.client_key_id,
        input.client_key_label,
        requested_model,
        upstream_model,
        input.endpoint_id,
        input.endpoint_key_id,
        usage_status,
        pricing_status,
        provider_cost,
        customer_amount,
        usage.map(|usage| usage.input_tokens).unwrap_or_default(),
        usage
            .map(|usage| usage.cache_read_tokens)
            .unwrap_or_default(),
        usage
            .map(|usage| usage.cache_write_tokens)
            .unwrap_or_default(),
        usage.map(|usage| usage.output_tokens).unwrap_or_default(),
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query_file!("src/sql/billing/delete_charge_lines.sql", charge_id)
        .execute(&mut *tx)
        .await?;
    if let Some(usage) = usage {
        insert_lines(
            &mut tx,
            charge_id,
            usage,
            sale_rule.as_ref(),
            cost_rule.as_ref(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_charges(
    pool: &PgPool,
    filter: &BillingChargeFilter,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<BillingChargeRow>)> {
    let total = sqlx::query_file_scalar!(
        "src/sql/billing/count_charges.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
    )
    .fetch_one(pool)
    .await?;
    let charges = sqlx::query_file_as!(
        BillingChargeRow,
        "src/sql/billing/list_charges.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
        rows,
        first,
    )
    .fetch_all(pool)
    .await?;
    Ok((total, charges))
}

pub async fn get_charge(pool: &PgPool, charge_id: i64) -> Result<Option<BillingChargeDetail>> {
    let Some(charge) = sqlx::query_file_as!(
        BillingChargeRow,
        "src/sql/billing/get_charge.sql",
        charge_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let lines = sqlx::query_file_as!(
        BillingChargeLineRow,
        "src/sql/billing/list_charge_lines.sql",
        charge_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(Some(BillingChargeDetail { charge, lines }))
}

pub async fn billing_summary(
    pool: &PgPool,
    filter: &BillingChargeFilter,
) -> Result<BillingSummary> {
    let summary = sqlx::query_file_as!(
        BillingSummaryRow,
        "src/sql/billing/summary.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
    )
    .fetch_one(pool)
    .await?;
    let by_client_key = sqlx::query_file_as!(
        BillingBreakdownRow,
        "src/sql/billing/breakdown_client_key.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
        100_i64,
    )
    .fetch_all(pool)
    .await?;
    let by_model = sqlx::query_file_as!(
        BillingBreakdownRow,
        "src/sql/billing/breakdown_model.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
        100_i64,
    )
    .fetch_all(pool)
    .await?;
    Ok(BillingSummary {
        summary,
        by_client_key,
        by_model,
    })
}

pub async fn list_charge_export(
    pool: &PgPool,
    filter: &BillingChargeFilter,
) -> Result<Vec<BillingExportRow>> {
    Ok(sqlx::query_file_as!(
        BillingExportRow,
        "src/sql/billing/export_charges.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn list_monthly_export(
    pool: &PgPool,
    filter: &BillingChargeFilter,
) -> Result<Vec<BillingMonthlyExportRow>> {
    Ok(sqlx::query_file_as!(
        BillingMonthlyExportRow,
        "src/sql/billing/export_monthly.sql",
        filter.user_id,
        filter.client_key_id,
        filter.requested_model,
        filter.endpoint_id,
        filter.usage_status,
        filter.pricing_status,
        filter.request_id,
        filter.start_at,
        filter.end_at,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn reprice_unpriced_charges(pool: &PgPool, limit: i64) -> Result<u64> {
    let rows = sqlx::query_file_as!(
        UnpricedChargeRow,
        "src/sql/billing/list_unpriced_charges.sql",
        limit.clamp(1, 10_000),
    )
    .fetch_all(pool)
    .await?;
    let mut changed = 0;
    for row in rows {
        let usage = NormalizedBillingUsage {
            input_tokens: row.input_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_write_tokens: row.cache_write_tokens,
            output_tokens: row.output_tokens,
        };
        let mut tx = pool.begin().await?;
        let sale_rule = match row.requested_model.as_deref() {
            Some(model) => match_sale_price_rule(&mut *tx, model, row.created_at)
                .await
                .with_context(|| {
                    format!(
                        "billing price lookup failed: charge_id={} price_side=sale \
                         public_model={model} endpoint_id={} upstream_model={} billing_at={}",
                        row.charge_id,
                        row.endpoint_id
                            .map_or_else(|| "<none>".to_string(), |id| id.to_string()),
                        row.upstream_model.as_deref().unwrap_or("<none>"),
                        row.created_at,
                    )
                })?,
            None => None,
        };
        let cost_rule = match (row.endpoint_id, row.upstream_model.as_deref()) {
            (Some(endpoint_id), Some(model)) => {
                match_cost_price_rule(&mut *tx, endpoint_id, model, row.created_at)
                    .await
                    .with_context(|| {
                        format!(
                            "billing price lookup failed: charge_id={} price_side=cost \
                             public_model={} endpoint_id={endpoint_id} upstream_model={model} \
                             billing_at={}",
                            row.charge_id,
                            row.requested_model.as_deref().unwrap_or("<none>"),
                            row.created_at,
                        )
                    })?
            }
            _ => None,
        };
        let Some(sale_rule) = sale_rule else {
            tx.rollback().await?;
            continue;
        };
        let Some(cost_rule) = cost_rule else {
            tx.rollback().await?;
            continue;
        };
        let provider_cost = amount_for_usage(&cost_rule, usage);
        let customer_amount = amount_for_usage(&sale_rule, usage);
        sqlx::query_file!(
            "src/sql/billing/reprice_charge.sql",
            row.charge_id,
            "priced",
            provider_cost,
            customer_amount,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query_file!("src/sql/billing/delete_charge_lines.sql", row.charge_id)
            .execute(&mut *tx)
            .await?;
        insert_lines(
            &mut tx,
            row.charge_id,
            usage,
            Some(&sale_rule),
            Some(&cost_rule),
        )
        .await?;
        tx.commit().await?;
        changed += 1;
    }
    Ok(changed)
}

fn normalized_usage(input: &RequestRecordCreate) -> Option<NormalizedBillingUsage> {
    NormalizedBillingUsage::from_usage(&crate::usage::TokenUsage {
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        total_tokens: input.total_tokens,
        cached_tokens: input.cached_tokens,
        cache_read_tokens: input.cache_read_tokens,
        cache_write_tokens: input.cache_write_tokens,
    })
}

fn amount_for_usage(rule: &BillingPriceRuleRow, usage: NormalizedBillingUsage) -> Decimal {
    let mut total = Decimal::ZERO;
    for meter in BillingMeter::ALL {
        total += Decimal::from(usage.token_count(meter)) * rule.rate(meter)
            / Decimal::from(TOKENS_PER_MILLION);
    }
    total
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    charge_id: i64,
    usage: NormalizedBillingUsage,
    sale_rule: Option<&BillingPriceRuleRow>,
    cost_rule: Option<&BillingPriceRuleRow>,
) -> Result<()> {
    for (side, rule) in [("sale", sale_rule), ("cost", cost_rule)] {
        for meter in BillingMeter::ALL {
            let token_count = usage.token_count(meter);
            let unit_rate = rule.map(|rule| rule.rate(meter)).unwrap_or(Decimal::ZERO);
            let amount = Decimal::from(token_count) * unit_rate / Decimal::from(TOKENS_PER_MILLION);
            sqlx::query_file!(
                "src/sql/billing/insert_charge_line.sql",
                charge_id,
                side,
                meter.as_str(),
                token_count,
                unit_rate,
                amount,
                rule.map(|rule| rule.price_rule_id),
            )
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use uuid::Uuid;

    use super::{TOKENS_PER_MILLION, amount_for_usage};
    use crate::db::types::{BillingPriceRuleRow, NormalizedBillingUsage};

    #[test]
    fn calculates_decimal_amounts_per_million_without_float_rounding() {
        let rule = BillingPriceRuleRow {
            price_rule_id: Uuid::nil(),
            price_side: "sale".to_string(),
            public_model: Some("gpt-test".to_string()),
            endpoint_id: None,
            upstream_model: None,
            input_rate: Decimal::from_str("0.125").unwrap(),
            cache_read_rate: Decimal::from_str("0.025").unwrap(),
            cache_write_rate: Decimal::from_str("0.05").unwrap(),
            output_rate: Decimal::from_str("0.5").unwrap(),
            currency: "CNY".to_string(),
            effective_from: Utc::now(),
            effective_to: None,
            enabled: true,
            created_by_user_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let usage = NormalizedBillingUsage {
            input_tokens: TOKENS_PER_MILLION,
            cache_read_tokens: TOKENS_PER_MILLION,
            cache_write_tokens: TOKENS_PER_MILLION,
            output_tokens: TOKENS_PER_MILLION,
        };

        assert_eq!(
            amount_for_usage(&rule, usage),
            Decimal::from_str("0.7").unwrap()
        );
    }

    #[test]
    fn charge_upsert_is_event_idempotent() {
        let sql = include_str!("../../sql/billing/upsert_charge.sql");
        assert!(sql.contains("ON CONFLICT (event_id)"));
    }
}
