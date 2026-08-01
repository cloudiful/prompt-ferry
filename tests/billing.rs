#[path = "support/billing_harness.rs"]
mod billing_harness;
#[path = "support/db_harness.rs"]
mod db_harness;

use billing_harness::{
    create_customer_price_rule, create_test_endpoint, create_test_user, create_unpriced_charge,
    migrated_schema,
};
use chrono::{Duration, Utc};
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
              'public_model', 'input_rate', 'cache_read_rate', 'cache_write_rate',
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
async fn reprice_unpriced_charge_uses_customer_price_rule() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let endpoint_id = create_test_endpoint(&schema.pool).await?;
    let user_id = create_test_user(&schema.pool).await?;
    let charge_id = create_unpriced_charge(&schema.pool, endpoint_id).await?;
    let price_rule_id = create_customer_price_rule(&schema.pool, user_id).await?;

    assert_eq!(db::reprice_unpriced_charges(&schema.pool, 10).await?, 1);
    let detail = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("repriced charge should still exist");
    assert_eq!(detail.charge.pricing_status, "priced");
    assert_eq!(detail.charge.customer_amount, Some(Decimal::from(9)));
    assert_eq!(detail.lines.len(), 4);
    assert!(
        detail
            .lines
            .iter()
            .all(|line| line.price_rule_id == Some(price_rule_id))
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
    let price_rule_id = create_customer_price_rule(&schema.pool, user_id).await?;

    sqlx::query("ALTER TABLE billing_price_rules ALTER COLUMN currency DROP NOT NULL")
        .execute(&schema.pool)
        .await?;
    sqlx::query("UPDATE billing_price_rules SET currency = NULL WHERE price_rule_id = $1")
        .bind(price_rule_id)
        .execute(&schema.pool)
        .await?;

    let error = db::reprice_unpriced_charges(&schema.pool, 10)
        .await
        .expect_err("a NULL currency must fail strict price-rule decoding");
    let message = format!("{error:#}");
    assert!(message.contains(&format!("charge_id={charge_id}")));
    assert!(message.contains("public_model=public-model"));
    assert!(message.contains("billing_at="));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn billing_migrations_remove_cost_pricing_and_keep_token_snapshots() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let adjusted_column_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'usage_charges'
          AND column_name = 'adjusted_amount'
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let adjustment_table_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema()
              AND table_name = 'usage_charge_adjustments'
        )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let pricing_constraint = sqlx::query_scalar::<_, String>(
        r#"
        SELECT pg_get_constraintdef(oid)
        FROM pg_constraint
        WHERE conname = 'ck_usage_charges_pricing_status'
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let snapshot_columns = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'usage_charges'
          AND column_name IN ('input_tokens', 'cache_read_tokens', 'cache_write_tokens', 'output_tokens')
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let price_rule_index_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM pg_indexes
            WHERE schemaname = current_schema()
              AND indexname = 'idx_usage_charge_lines_price_rule_id'
        )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let cost_column_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND (table_name, column_name) IN (
              ('usage_charges', 'provider_cost'),
              ('billing_price_rules', 'price_side'),
              ('billing_price_rules', 'endpoint_id'),
              ('billing_price_rules', 'upstream_model'),
              ('usage_charge_lines', 'price_side')
          )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;

    assert_eq!(adjusted_column_count, 0);
    assert!(!adjustment_table_exists);
    assert!(!pricing_constraint.contains("adjusted"));
    assert_eq!(snapshot_columns, 4);
    assert!(price_rule_index_exists);
    assert_eq!(cost_column_count, 0);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn billing_price_rule_update_preserves_id() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let user_id = create_test_user(&schema.pool).await?;
    let price_rule_id = create_customer_price_rule(&schema.pool, user_id).await?;
    let updated = db::update_price_rule(
        &schema.pool,
        price_rule_id,
        db::BillingPriceRuleUpdate {
            public_model: "renamed-public-model".to_string(),
            input_rate: Decimal::from(3),
            cache_read_rate: Decimal::from(4),
            cache_write_rate: Decimal::from(5),
            output_rate: Decimal::from(6),
            effective_from: Utc::now() - Duration::minutes(2),
        },
    )
    .await?
    .expect("price rule should exist");

    assert_eq!(updated.price_rule_id, price_rule_id);
    assert_eq!(updated.public_model, "renamed-public-model");
    assert_eq!(updated.input_rate, Decimal::from(3));
    assert_eq!(updated.output_rate, Decimal::from(6));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn deleting_unreferenced_price_rule_removes_only_the_rule() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let user_id = create_test_user(&schema.pool).await?;
    let endpoint_id = create_test_endpoint(&schema.pool).await?;
    let charge_id = create_unpriced_charge(&schema.pool, endpoint_id).await?;
    let price_rule_id = create_customer_price_rule(&schema.pool, user_id).await?;

    assert!(db::delete_price_rule(&schema.pool, price_rule_id).await?);
    assert!(
        db::list_price_rules(&schema.pool, 100, 0)
            .await?
            .iter()
            .all(|rule| rule.price_rule_id != price_rule_id)
    );
    assert!(db::get_charge(&schema.pool, charge_id).await?.is_some());
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn deleting_referenced_price_rule_resets_charge_and_allows_repricing() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let user_id = create_test_user(&schema.pool).await?;
    let endpoint_id = create_test_endpoint(&schema.pool).await?;
    let charge_id = create_unpriced_charge(&schema.pool, endpoint_id).await?;
    let price_rule_id = create_customer_price_rule(&schema.pool, user_id).await?;

    assert_eq!(db::reprice_unpriced_charges(&schema.pool, 10).await?, 1);
    assert!(db::delete_price_rule(&schema.pool, price_rule_id).await?);
    let reset = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("charge record should remain after deleting a price rule");
    assert_eq!(reset.charge.pricing_status, "unpriced");
    assert_eq!(reset.charge.customer_amount, None);
    assert!(reset.lines.is_empty());
    assert_eq!(reset.charge.input_tokens, 1_000_000);
    assert_eq!(reset.charge.cache_read_tokens, 0);
    assert_eq!(reset.charge.cache_write_tokens, 0);
    assert_eq!(reset.charge.output_tokens, 2_000_000);

    let replacement = db::create_price_rule(
        &schema.pool,
        db::BillingPriceRuleCreate {
            public_model: "public-model".to_string(),
            input_rate: Decimal::from(2),
            cache_read_rate: Decimal::ZERO,
            cache_write_rate: Decimal::ZERO,
            output_rate: Decimal::from(5),
            effective_from: Utc::now() - Duration::minutes(1),
            created_by_user_id: user_id,
        },
    )
    .await?;
    assert_ne!(replacement.price_rule_id, price_rule_id);
    assert_eq!(db::reprice_unpriced_charges(&schema.pool, 10).await?, 1);
    let repriced = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("charge should be repriced");
    assert_eq!(repriced.charge.pricing_status, "priced");
    assert_eq!(repriced.lines.len(), 4);
    assert!(
        repriced
            .lines
            .iter()
            .all(|line| line.price_rule_id == Some(replacement.price_rule_id))
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn deleting_missing_price_rule_reports_not_found() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping billing database test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    assert!(!db::delete_price_rule(&schema.pool, Uuid::new_v4()).await?);
    schema.cleanup().await?;
    Ok(())
}
