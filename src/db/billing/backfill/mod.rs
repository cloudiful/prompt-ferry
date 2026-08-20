//! Historical usage-token backfill for `request_records`.
//!
//! Old Anthropic-compatible responses stored `input_tokens=176, cached=82793`
//! while the raw SSE carried the provider-native `cache_read_input_tokens` value.
//! Phase 1 unified parsing across OpenAI and Anthropic usage, and Phase 2 fixed
//! the cache-rate SQL. This module provides the controlled, idempotent path that
//! repairs historical records from their retained raw payloads and refreshes
//! the matching `usage_charges` snapshot and `usage_charge_lines`.
//!
//! The backfill is dry-run-first: nothing is written unless the caller passes
//! `apply = true`. It is bounded by `--limit`, an optional `--max-batches`, a
//! key-set cursor (`after_event_id`), and optional `--since`/`--until` time
//! window arguments. Records whose raw payload is only available through
//! object-store metadata (no PG body) are reported as `skipped` rather than
//! guessed; records whose raw payload is present but cannot be parsed, or
//! whose apply transaction fails, are reported as `failed`.
//!
//! Safety guards:
//! * `response_capture_truncated = true` rows are skipped before any raw
//!   load so a partial body cannot erase existing tokens.
//! * Unknown parsed token fields (None on the parsed side) do not overwrite
//!   stored values, both via `COALESCE` in SQL and `(Some(_), None) == false`
//!   in `field_changed`.
//! * DB errors during the raw body load surface as `Failed` (never silently
//!   demoted to `Skipped`).
//! * An already-priced charge never silently becomes `unpriced`; the apply
//!   transaction aborts when the historical price rule is missing.

mod apply;
mod parser;
#[cfg(test)]
mod tests;
mod types;

pub use parser::parse_raw_response;
pub use types::{
    BackfillBatchOutcome, BackfillCandidate, BackfillDecision, BackfillOptions, BackfillOutcome,
    BackfillStats, StatsBucket, billing_lookup_key, classify_outcome, decide_repair,
};

use anyhow::{Context, Result};
use sqlx::PgPool;

use apply::{apply_repair, load_response_body};

/// Reason string for rows skipped because the upstream capture was truncated.
pub const SKIPPED_TRUNCATED_REASON: &str = "response capture truncated; raw usage is incomplete";

/// Run one bounded batch of the backfill. The scan is keyset-paginated by
/// `event_id > options.after_event_id`; callers should pass the largest
/// `event_id` from the previous batch as the next cursor. Returns both the
/// aggregated stats and the largest `event_id` actually observed so the
/// caller can advance the cursor without re-reading rows.
///
/// `BackfillBatchOutcome::diagnostics` carries every `Failed` outcome plus
/// a representative subset of `Skipped` outcomes so the operator can see
/// which events need follow-up without us holding unbounded state.
pub async fn backfill_token_usage(
    pool: &PgPool,
    options: BackfillOptions,
) -> Result<BackfillBatchOutcome> {
    let candidates = sqlx::query_file_as!(
        BackfillCandidate,
        "src/sql/billing/list_request_records_for_token_backfill.sql",
        options.since,
        options.until,
        options.limit,
        options.after_event_id,
    )
    .fetch_all(pool)
    .await
    .context("failed to load request_record backfill candidates")?;

    let mut stats = BackfillStats::default();
    let mut last_event_id = options.after_event_id;
    let mut diagnostics: Vec<BackfillOutcome> = Vec::new();
    for candidate in candidates {
        stats.scanned += 1;
        let outcome = process_candidate(pool, &candidate, options.apply).await;
        match classify_outcome(&outcome) {
            StatsBucket::Repaired => stats.repaired += 1,
            StatsBucket::Unchanged => stats.unchanged += 1,
            StatsBucket::Skipped => {
                stats.skipped += 1;
                diagnostics.push(outcome.clone());
            }
            StatsBucket::Failed => {
                stats.failed += 1;
                diagnostics.push(outcome.clone());
            }
        }
        // Cursor advances even on Skipped and Failed outcomes so a single
        // bad event never wedges the run.
        last_event_id = candidate.event_id;
    }
    Ok(BackfillBatchOutcome {
        stats,
        last_event_id,
        diagnostics,
    })
}

async fn process_candidate(
    pool: &PgPool,
    candidate: &BackfillCandidate,
    apply: bool,
) -> BackfillOutcome {
    // Guard 1: refuse to overwrite rows whose upstream capture was truncated.
    // A truncated body could carry a partial usage block that would erase
    // valid totals when folded through the canonical parser.
    if candidate.response_capture_truncated {
        return BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Skipped,
            reason: Some(SKIPPED_TRUNCATED_REASON.to_string()),
        };
    }
    // Guard 2: object-store-only or fully-missing raw bodies stay skipped.
    if !candidate.response_in_postgres && !candidate.request_in_postgres {
        return BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Skipped,
            reason: Some(if candidate.raw_object_only {
                "raw payload only in object store; object fetch not configured".to_string()
            } else {
                "raw payload unavailable".to_string()
            }),
        };
    }
    // Guard 3: a DB error during the raw load must surface as Failed, not
    // be silently demoted to Skipped by `.ok().flatten()`.
    let raw_response =
        match load_response_body(pool, candidate.event_id, candidate.created_at).await {
            Ok(Some(body)) => body,
            Ok(None) => {
                return BackfillOutcome {
                    event_id: candidate.event_id,
                    decision: BackfillDecision::Skipped,
                    reason: Some("response body missing despite candidate flag".to_string()),
                };
            }
            Err(error) => {
                return BackfillOutcome {
                    event_id: candidate.event_id,
                    decision: BackfillDecision::Failed,
                    reason: Some(format!("load-failed: {error}")),
                };
            }
        };
    let Some(parsed) = parse_raw_response(&raw_response) else {
        return BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Failed,
            reason: Some(
                "parse-failed: no usage block recovered from retained response".to_string(),
            ),
        };
    };
    if !decide_repair(candidate, &parsed) {
        return BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Unchanged,
            reason: None,
        };
    }
    if !apply {
        return BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Repaired,
            reason: Some("dry-run: repair planned but not applied".to_string()),
        };
    }
    match apply_repair(pool, candidate, &parsed).await {
        Ok(()) => BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Repaired,
            reason: None,
        },
        Err(error) => BackfillOutcome {
            event_id: candidate.event_id,
            decision: BackfillDecision::Failed,
            // The error chain from `apply_repair` may include DB driver
            // detail. Trim anything that looks like a raw payload reference
            // before exposing it to the operator.
            reason: Some(scrub_apply_error(&format!("{error:#}"), candidate.event_id)),
        },
    }
}

/// Replace any large payload-shaped content in an apply error with a
/// short, operator-safe summary. The current `apply_repair` errors never
/// embed payload bytes — the trim is defensive in case a future call site
/// adds a `?`-propagated error that does.
fn scrub_apply_error(error: &str, event_id: i64) -> String {
    let max_chars = 200;
    let trimmed: String = error.chars().take(max_chars).collect();
    if error.chars().count() > max_chars {
        format!("apply-failed: event_id={event_id} {trimmed} …(truncated)")
    } else {
        format!("apply-failed: event_id={event_id} {trimmed}")
    }
}
