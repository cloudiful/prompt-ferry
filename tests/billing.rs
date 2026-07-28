#[path = "support/billing_harness.rs"]
mod billing_harness;
#[path = "support/db_harness.rs"]
mod db_harness;

use billing_harness::{
    create_sale_and_cost_rules, create_test_endpoint, create_test_user, create_unpriced_charge,
    migrated_schema,
};
use prompt_ferry::db;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, test_database_configured};

#[tokio::test]
async fn billing_price_rule_required_columns_are_not_nullable() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let not_null_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'billing_price_rules'
          AND is_nullable = 'NO'
          AND column_name IN (
              'price_side', 'input_rate', 'cache_read_rate', 'cache_write_rate',
              'output_rate', 'currency', 'effective_from', 'enabled', 'created_at',
              'updated_at'
          )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(not_null_count, 10);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reprice_unpriced_charge_uses_sale_and_cost_rules() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let endpoint_id = create_test_endpoint(&schema.pool).await?;
    let user_id = create_test_user(&schema.pool).await?;
    let charge_id = create_unpriced_charge(&schema.pool, endpoint_id).await?;
    let (sale_rule_id, cost_rule_id) =
        create_sale_and_cost_rules(&schema.pool, endpoint_id, user_id).await?;

    assert_eq!(db::reprice_unpriced_charges(&schema.pool, 10).await?, 1);
    let detail = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("repriced charge should still exist");
    assert_eq!(detail.charge.pricing_status, "priced");
    assert_eq!(detail.charge.customer_amount, Some(Decimal::from(9)));
    assert_eq!(detail.charge.provider_cost, Some(Decimal::new(45, 1)));
    assert_eq!(detail.lines.len(), 8);
    assert!(
        detail
            .lines
            .iter()
            .filter(|line| line.price_side == "sale")
            .all(|line| line.price_rule_id == Some(sale_rule_id))
    );
    assert!(
        detail
            .lines
            .iter()
            .filter(|line| line.price_side == "cost")
            .all(|line| line.price_rule_id == Some(cost_rule_id))
    );
    assert_eq!(db::reprice_unpriced_charges(&schema.pool, 10).await?, 0);

    sqlx::query("UPDATE usage_charges SET user_id = NULL, endpoint_id = NULL WHERE charge_id = $1")
        .bind(charge_id)
        .execute(&schema.pool)
        .await?;
    let filter = db::BillingChargeFilter {
        user_id: None,
        client_key_id: None,
        requested_model: None,
        endpoint_id: None,
        usage_status: None,
        pricing_status: None,
        request_id: None,
        start_at: None,
        end_at: None,
    };
    let (_, charges) = db::list_charges(&schema.pool, &filter, 0, 10).await?;
    let listed_charge = charges
        .iter()
        .find(|charge| charge.charge_id == charge_id)
        .expect("charge with nullable joined fields should be listed");
    assert_eq!(listed_charge.user_login_name, None);
    assert_eq!(listed_charge.endpoint_name, None);
    let exports = db::list_charge_export(&schema.pool, &filter).await?;
    let exported_charge = exports
        .iter()
        .find(|charge| charge.charge_id == charge_id)
        .expect("charge with nullable joined fields should be exported");
    assert_eq!(exported_charge.user_login_name, None);
    assert_eq!(exported_charge.endpoint_name, None);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reprice_reports_charge_and_price_lookup_context_on_decode_error() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let endpoint_id = create_test_endpoint(&schema.pool).await?;
    let user_id = create_test_user(&schema.pool).await?;
    let charge_id = create_unpriced_charge(&schema.pool, endpoint_id).await?;
    let (sale_rule_id, _) = create_sale_and_cost_rules(&schema.pool, endpoint_id, user_id).await?;

    sqlx::query("ALTER TABLE billing_price_rules ALTER COLUMN currency DROP NOT NULL")
        .execute(&schema.pool)
        .await?;
    sqlx::query("UPDATE billing_price_rules SET currency = NULL WHERE price_rule_id = $1")
        .bind(sale_rule_id)
        .execute(&schema.pool)
        .await?;

    let error = db::reprice_unpriced_charges(&schema.pool, 10)
        .await
        .expect_err("a NULL currency must fail strict price-rule decoding");
    let message = format!("{error:#}");
    assert!(message.contains(&format!("charge_id={charge_id}")));
    assert!(message.contains("price_side=sale"));
    assert!(message.contains("public_model=public-model"));
    assert!(message.contains(&format!("endpoint_id={endpoint_id}")));
    assert!(message.contains("upstream_model=upstream-model"));
    assert!(message.contains("billing_at="));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn billing_price_rule_migration_rejects_null_rows_with_rule_id() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    sqlx::raw_sql(
        r#"
        ALTER TABLE billing_price_rules
            ALTER COLUMN price_side DROP NOT NULL,
            ALTER COLUMN input_rate DROP NOT NULL,
            ALTER COLUMN cache_read_rate DROP NOT NULL,
            ALTER COLUMN cache_write_rate DROP NOT NULL,
            ALTER COLUMN output_rate DROP NOT NULL,
            ALTER COLUMN currency DROP NOT NULL,
            ALTER COLUMN effective_from DROP NOT NULL,
            ALTER COLUMN enabled DROP NOT NULL,
            ALTER COLUMN created_at DROP NOT NULL,
            ALTER COLUMN updated_at DROP NOT NULL
        "#,
    )
    .execute(&schema.pool)
    .await?;
    let price_rule_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO billing_price_rules (
            price_rule_id, price_side, public_model, input_rate, cache_read_rate,
            cache_write_rate, output_rate, currency, effective_from, enabled,
            created_at, updated_at
        )
        VALUES ($1, 'sale', 'invalid-model', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)
        "#,
    )
    .bind(price_rule_id)
    .execute(&schema.pool)
    .await?;

    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = $1")
        .bind(51_i64)
        .execute(&schema.pool)
        .await?;
    let error = db::migrate(&schema.pool)
        .await
        .expect_err("the migration must reject rows with required NULLs");
    let message = format!("{error:#}");
    assert!(message.contains(&price_rule_id.to_string()));
    assert!(message.contains("currency"));
    assert!(message.contains("effective_from"));
    schema.cleanup().await?;
    Ok(())
}
