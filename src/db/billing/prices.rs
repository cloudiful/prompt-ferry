use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{BillingPriceRuleCreate, BillingPriceRuleRow};

pub async fn list_price_rules(
    pool: &PgPool,
    first: i64,
    offset: i64,
) -> Result<Vec<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/list_price_rules.sql",
        first,
        offset,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn count_price_rules(pool: &PgPool) -> Result<i64> {
    Ok(
        sqlx::query_file_scalar!("src/sql/billing/count_price_rules.sql")
            .fetch_one(pool)
            .await?,
    )
}

pub async fn create_price_rule(
    pool: &PgPool,
    input: BillingPriceRuleCreate,
) -> Result<BillingPriceRuleRow> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/create_price_rule.sql",
        input.price_side.as_str(),
        input.public_model,
        input.endpoint_id,
        input.upstream_model,
        input.input_rate,
        input.cache_read_rate,
        input.cache_write_rate,
        input.output_rate,
        input.effective_from,
        input.created_by_user_id,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_price_rule_status(
    pool: &PgPool,
    price_rule_id: Uuid,
    enabled: bool,
) -> Result<Option<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/update_price_rule_status.sql",
        price_rule_id,
        enabled,
    )
    .fetch_optional(pool)
    .await?)
}

pub(super) async fn match_sale_price_rule(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    public_model: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<Option<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/match_sale_price_rule.sql",
        public_model,
        at,
    )
    .fetch_optional(executor)
    .await?)
}

pub(super) async fn match_cost_price_rule(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    endpoint_id: Uuid,
    upstream_model: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<Option<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/match_cost_price_rule.sql",
        endpoint_id,
        upstream_model,
        at,
    )
    .fetch_optional(executor)
    .await?)
}
