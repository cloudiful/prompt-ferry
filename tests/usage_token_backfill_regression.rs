//! Regression test: a priced charge whose price rule is no longer available
//! must not be silently downgraded to `unpriced`. Loaded from
//! `tests/usage_token_backfill_safety.rs` via `#[path = "..."] mod regression;`.

#[path = "support/billing_harness.rs"]
mod billing_harness;
#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "usage_token_backfill_helpers.rs"]
mod helpers;

use billing_harness::migrated_schema;
use db_harness::{TEST_DATABASE_URL_ENV, test_database_configured};
use helpers::{ANTHROPIC_SSE_RESPONSE, insert_old_anthropic, run_batch, stored_tokens};
use prompt_ferry::db::{self, BackfillOptions, BillingPriceRuleRow};
use std::collections::HashMap;

/// Reads the charge_id for one event (or `None` when no charge exists).
async fn charge_id_for_event(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<Option<i64>> {
    let row: Option<i64> = sqlx::query_file_scalar!(
        "tests/sql/usage_backfill/get_charge_id_by_event.sql",
        event_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Regression test: a priced charge whose originating price rule is no longer
/// available must not be silently downgraded to `unpriced`. The backfill
/// must surface this as a `Failed` outcome, the per-event transaction must
/// roll back, and the historical charge (status, amount, lines) must remain
/// exactly as it was before the backfill pass.
#[tokio::test]
async fn priced_charge_with_missing_rule_is_not_downgraded() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let user_id = billing_harness::create_test_user(&schema.pool).await?;
    let price_rule_id = billing_harness::create_customer_price_rule(&schema.pool, user_id).await?;

    // Insert the request record WITH the rule still active. `record_request_record`
    // auto-runs `record_usage_charge`, which finds the rule and creates a
    // priced charge anchored to `price_rule_id`.
    let event_id =
        insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    let charge_id = charge_id_for_event(&schema.pool, event_id)
        .await?
        .expect("record_usage_charge must have created a priced charge");
    let before_charge = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("charge exists for ai event");
    assert_eq!(before_charge.charge.pricing_status, "priced");
    assert!(
        before_charge
            .lines
            .iter()
            .all(|line| line.price_rule_id == Some(price_rule_id)),
        "every priced line must be anchored to the rule we created, got {:?}",
        before_charge.lines
    );
    let before_lines: HashMap<String, i64> = before_charge
        .lines
        .iter()
        .map(|line| (line.meter.clone(), line.token_count))
        .collect();
    let before_amount = before_charge.charge.customer_amount;
    let before_tokens = stored_tokens(&schema.pool, event_id).await?;

    // Disable the rule so `match_price_rule` returns None. The historical
    // priced charge stays intact.
    let updated_rule: BillingPriceRuleRow =
        db::update_price_rule_status(&schema.pool, price_rule_id, false)
            .await?
            .expect("update_price_rule_status must return the row");
    assert!(!updated_rule.enabled, "rule must be disabled for the test");

    // Backfill with apply=true. The row still has the old-priced split (the
    // canonical parser folds cache_read into a higher input), so
    // `decide_repair` returns true and the apply path tries to refresh the
    // charge. The price-rule lookup will fail (rule disabled) AND the
    // existing charge is still priced, so the apply transaction must abort.
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert!(
        outcome.stats.failed >= 1,
        "expected at least one failed outcome for the priced-without-rule row, \
         got stats={:?}, diagnostics={:?}",
        outcome.stats,
        outcome.diagnostics
    );
    let failed = outcome
        .diagnostics
        .iter()
        .find(|d| d.event_id == event_id)
        .expect("the priced-without-rule row must appear in diagnostics");
    assert!(matches!(failed.decision, db::BackfillDecision::Failed));
    let reason = failed.reason.as_deref().unwrap_or("");
    assert!(
        reason.contains("apply-failed") && reason.contains("historical price rule unavailable"),
        "diagnostic reason must call out the missing-rule regression, got {:?}",
        reason
    );

    // The per-event transaction must have rolled back. request_records tokens
    // stay at the pre-backfill values.
    let after_tokens = stored_tokens(&schema.pool, event_id).await?;
    assert_eq!(
        after_tokens, before_tokens,
        "request_records token fields must be unchanged after the failed apply"
    );

    // The historical charge must remain priced at the same amount with the
    // same per-meter line tokens.
    let after_charge = db::get_charge(&schema.pool, charge_id)
        .await?
        .expect("charge still exists after failed apply");
    assert_eq!(after_charge.charge.pricing_status, "priced");
    assert_eq!(
        after_charge.charge.customer_amount, before_amount,
        "customer_amount must not be cleared by the failed apply"
    );
    let after_lines: HashMap<String, i64> = after_charge
        .lines
        .iter()
        .map(|line| (line.meter.clone(), line.token_count))
        .collect();
    assert_eq!(
        after_lines, before_lines,
        "charge lines must not be cleared or rewritten by the failed apply"
    );
    assert!(
        after_charge
            .lines
            .iter()
            .all(|line| line.price_rule_id == Some(price_rule_id)),
        "lines must remain anchored to the original price rule"
    );

    schema.cleanup().await?;
    Ok(())
}
