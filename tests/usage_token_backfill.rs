#[path = "support/billing_harness.rs"]
mod billing_harness;
#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "usage_token_backfill_helpers.rs"]
mod helpers;
#[path = "usage_token_backfill_safety.rs"]
mod safety_tests;

use billing_harness::migrated_schema;
use db_harness::{TEST_DATABASE_URL_ENV, test_database_configured};
use helpers::{ANTHROPIC_SSE_RESPONSE, insert_old_anthropic, run_batch, stored_tokens};
use prompt_ferry::db::{self, BackfillOptions, BackfillStats};

#[tokio::test]
async fn dry_run_reports_repair_without_writing() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let event_id =
        insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    let before = stored_tokens(&schema.pool, event_id).await?;
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: false,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert!(outcome.stats.scanned >= 1);
    assert!(outcome.stats.repaired >= 1);
    assert_eq!(outcome.stats.failed, 0);
    assert_eq!(
        stored_tokens(&schema.pool, event_id).await?,
        before,
        "dry-run must not modify rows"
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn apply_repairs_tokens_and_refreshes_charges() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let user_id = billing_harness::create_test_user(&schema.pool).await?;
    let price_rule_id = billing_harness::create_customer_price_rule(&schema.pool, user_id).await?;
    let event_id =
        insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert!(outcome.stats.repaired >= 1);
    let after = stored_tokens(&schema.pool, event_id).await?;
    // request_records canonical totals: ordinary + cache_read + cache_write.
    assert_eq!(after.input_tokens, Some(82976));
    assert_eq!(after.cache_read_tokens, Some(82793));
    assert_eq!(after.cache_write_tokens, Some(7));
    assert_eq!(after.output_tokens, Some(42));
    let detail = db::get_charge(&schema.pool, event_id)
        .await?
        .expect("charge row exists for ai event");
    assert_eq!(detail.charge.usage_status, "known");
    assert_eq!(detail.charge.pricing_status, "priced");
    assert!(
        detail.charge.customer_amount.is_some(),
        "priced charge must carry a non-null customer_amount, got {:?}",
        detail.charge.customer_amount
    );
    assert_eq!(detail.charge.input_tokens, 176);
    assert_eq!(detail.charge.cache_read_tokens, 82793);
    assert_eq!(detail.charge.cache_write_tokens, 7);
    assert_eq!(detail.charge.output_tokens, 42);
    // Per-meter billing split: every canonical BillingMeter must have a
    // matching line, with the token_count values that come from the same
    // parser pipeline the production billing path uses.
    assert_eq!(
        detail.lines.len(),
        4,
        "expected one line per canonical billing meter, got {:?}",
        detail.lines
    );
    let mut expected_by_meter = std::collections::HashMap::new();
    expected_by_meter.insert("input".to_string(), 176_i64);
    expected_by_meter.insert("cache_read".to_string(), 82793_i64);
    expected_by_meter.insert("cache_write".to_string(), 7_i64);
    expected_by_meter.insert("output".to_string(), 42_i64);
    let mut seen_by_meter = std::collections::HashMap::new();
    for line in &detail.lines {
        seen_by_meter.insert(line.meter.clone(), line.token_count);
        assert_eq!(
            line.price_rule_id,
            Some(price_rule_id),
            "every priced line must be anchored to the rule we created, got {:?}",
            line
        );
    }
    assert_eq!(
        seen_by_meter, expected_by_meter,
        "per-meter token counts must match the canonical billing split"
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn second_apply_is_idempotent_and_unchanged() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    let first = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    let second = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert!(first.stats.repaired >= 1);
    assert_eq!(second.stats.repaired, 0);
    assert!(second.stats.unchanged >= 1);
    assert_eq!(second.stats.failed, 0);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn cursor_advances_only_through_processed_event_ids() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let first = insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    let second = insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    assert!(first < second, "fixtures must produce ascending event_ids");
    let first_batch = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: false,
            limit: 1,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(first_batch.stats.scanned, 1);
    assert_eq!(first_batch.last_event_id, first);
    let second_batch = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: false,
            limit: 50,
            after_event_id: first_batch.last_event_id,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(second_batch.stats.scanned, 1);
    assert_eq!(second_batch.last_event_id, second);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn classify_outcome_separates_failed_from_skipped() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let mut a = BackfillStats::default();
    a.scanned = 1;
    a.failed = 1;
    let mut b = BackfillStats::default();
    b.scanned = 1;
    b.skipped = 1;
    a.add(b);
    assert_eq!(a.failed, 1);
    assert_eq!(a.skipped, 1);
    assert!(!a.is_clean());
    Ok(())
}
