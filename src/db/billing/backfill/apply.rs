//! Apply-path helpers for the historical backfill.
//!
//! Splitting these out keeps `mod.rs` focused on the public API while
//! isolating the database write logic and the per-event charge refresh in
//! their own file. Both helpers are crate-private; the public surface lives
//! in `super`.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::billing::charges::{amount_for_usage, insert_lines, normalized_usage_from_fields};
use crate::db::billing::prices::match_price_rule;
use crate::db::types::{BillingPriceRuleRow, NormalizedBillingUsage};
use crate::usage::TokenUsage;

use super::{BackfillCandidate, billing_lookup_key};

/// Loads the retained response body for one `(event_id, created_at)` pair.
///
/// Returns `Ok(None)` when the row exists but the body column is NULL (raw
/// missing on disk, object-only payload, etc.). Returns `Err(...)` when the
/// database call fails — callers must surface DB errors as `Failed`
/// outcomes instead of silently masking them.
pub(super) async fn load_response_body(
    pool: &PgPool,
    event_id: i64,
    created_at: DateTime<Utc>,
) -> Result<Option<String>> {
    let row = sqlx::query_file!(
        "src/sql/billing/get_request_record_response_body.sql",
        event_id,
        created_at,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch retained response body")?;
    Ok(row.and_then(|row| row.response_raw_body))
}

pub(super) async fn apply_repair(
    pool: &PgPool,
    candidate: &BackfillCandidate,
    parsed: &TokenUsage,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query_file!(
        "src/sql/billing/update_request_record_tokens.sql",
        candidate.event_id,
        parsed.input_tokens,
        parsed.output_tokens,
        parsed.total_tokens,
        parsed.cached_tokens,
        parsed.cache_read_tokens,
        parsed.cache_write_tokens,
    )
    .execute(&mut *tx)
    .await
    .context("failed to update request_records token fields")?;
    let usage = normalized_usage_from_fields(
        parsed.input_tokens,
        parsed.output_tokens,
        parsed.cached_tokens,
        parsed.cache_read_tokens,
        parsed.cache_write_tokens,
    );
    refresh_charge_snapshot(&mut tx, candidate, usage).await?;
    tx.commit().await?;
    Ok(())
}

/// Refreshes the `usage_charges` row and its lines for the event. Mirrors the
/// pricing semantics of `record_usage_charge` with one extra safety check: if
/// the existing charge row is already `priced` and no `billing_price_rules`
/// row can be matched, we return an error rather than silently re-pricing it
/// as `unpriced`. That keeps the historical price snapshot intact when a
/// rule has been retired since the original billing write.
///
/// For rows that were never priced (or that have no `usage_charges` row at
/// all), the `unpriced` branch remains the default so the backfill can fill
/// in new pricing when a rule exists.
async fn refresh_charge_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    candidate: &BackfillCandidate,
    usage: Option<NormalizedBillingUsage>,
) -> Result<()> {
    let usage_status = if usage.is_some() { "known" } else { "unknown" };
    // Mirrors `record_usage_charge`: try `requested_model` first, then fall
    // back to the resolved `request_records.model`. A row with neither stays
    // unpriced.
    let lookup_model = billing_lookup_key(candidate);
    let upstream_model = candidate.upstream_model.clone();
    let at = sqlx::query_file_scalar!(
        "src/sql/billing/charge_pricing_time.sql",
        candidate.event_id,
    )
    .fetch_one(&mut **tx)
    .await?;
    let price_rule: Option<BillingPriceRuleRow> = match lookup_model.as_deref() {
        Some(model) => match_price_rule(&mut **tx, model, at).await?,
        None => None,
    };
    // Safety check: never silently downgrade an already-priced charge. If we
    // could not find a price rule for the model and the existing charge row
    // says it was priced, we must abort the transaction so the operator can
    // investigate the missing rule rather than lose the historical amount.
    let existing_pricing_status: Option<String> = sqlx::query_file_scalar!(
        "src/sql/billing/get_charge_pricing_state.sql",
        candidate.event_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    if price_rule.is_none() && matches!(existing_pricing_status.as_deref(), Some("priced")) {
        return Err(anyhow!(
            "historical price rule unavailable for already-priced charge: \
             event_id={} lookup_model={}",
            candidate.event_id,
            lookup_model.as_deref().unwrap_or("<none>"),
        ));
    }
    let priced = usage.is_some() && price_rule.is_some();
    let pricing_status = if priced { "priced" } else { "unpriced" };
    let customer_amount = usage
        .zip(price_rule.as_ref())
        .map(|(usage, rule)| amount_for_usage(rule, usage));
    let charge_id = sqlx::query_file_scalar!(
        "src/sql/billing/upsert_charge.sql",
        candidate.event_id,
        candidate.request_id,
        Option::<i64>::None,
        Option::<i64>::None,
        Option::<String>::None,
        lookup_model,
        upstream_model,
        Option::<Uuid>::None,
        Option::<Uuid>::None,
        usage_status,
        pricing_status,
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
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query_file!("src/sql/billing/delete_charge_lines.sql", charge_id)
        .execute(&mut **tx)
        .await?;
    if let Some(usage) = usage {
        insert_lines(tx, charge_id, usage, price_rule.as_ref()).await?;
    }
    Ok(())
}
