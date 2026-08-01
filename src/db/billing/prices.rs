use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{BillingPriceRuleCreate, BillingPriceRuleRow, BillingPriceRuleUpdate};

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
        input.public_model,
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

pub async fn update_price_rule(
    pool: &PgPool,
    price_rule_id: Uuid,
    input: BillingPriceRuleUpdate,
) -> Result<Option<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/update_price_rule.sql",
        price_rule_id,
        input.public_model,
        input.input_rate,
        input.cache_read_rate,
        input.cache_write_rate,
        input.output_rate,
        input.effective_from,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_price_rule(pool: &PgPool, price_rule_id: Uuid) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let exists = sqlx::query_file_scalar!("src/sql/billing/lock_price_rule.sql", price_rule_id,)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query_file!(
        "src/sql/billing/reset_price_rule_charges.sql",
        price_rule_id,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "src/sql/billing/delete_price_rule_charge_lines.sql",
        price_rule_id,
    )
    .execute(&mut *tx)
    .await?;
    let result = sqlx::query_file!("src/sql/billing/delete_price_rule.sql", price_rule_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub(super) async fn match_price_rule(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    public_model: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<Option<BillingPriceRuleRow>> {
    Ok(sqlx::query_file_as!(
        BillingPriceRuleRow,
        "src/sql/billing/match_price_rule.sql",
        public_model,
        at,
    )
    .fetch_optional(executor)
    .await
    .with_context(|| {
        format!("billing price rule lookup failed: public_model={public_model} billing_at={at}")
    })?)
}
