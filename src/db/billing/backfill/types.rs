//! Public types for the historical usage-token backfill.
//!
//! Splitting the type definitions out keeps `mod.rs` focused on the run-loop
//! while still letting callers reach for `BackfillOptions`, `BackfillStats`,
//! `BackfillDecision`, `BackfillCandidate`, `BackfillOutcome`,
//! `BackfillBatchOutcome`, `StatsBucket`, `decide_repair`,
//! `billing_lookup_key`, and `classify_outcome` from the same module path.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::usage::TokenUsage;

/// Bounded input parameters for one backfill run.
#[derive(Debug, Clone, Copy)]
pub struct BackfillOptions {
    /// When false (default), nothing is written.
    pub apply: bool,
    /// Maximum rows to inspect per batch.
    pub limit: i64,
    /// Optional creation-time lower bound.
    pub since: Option<DateTime<Utc>>,
    /// Optional creation-time upper bound (exclusive).
    pub until: Option<DateTime<Utc>>,
    /// Key-set cursor: only rows with `event_id > after_event_id` are read.
    /// Use the largest `event_id` from the previous batch to make forward
    /// progress; `Some(0)` starts at the beginning.
    pub after_event_id: i64,
}

impl Default for BackfillOptions {
    fn default() -> Self {
        Self {
            apply: false,
            limit: 500,
            since: None,
            until: None,
            after_event_id: 0,
        }
    }
}

/// Counts returned to the operator after a backfill run completes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackfillStats {
    pub scanned: usize,
    pub repaired: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl BackfillStats {
    pub fn add(&mut self, other: BackfillStats) {
        self.scanned += other.scanned;
        self.repaired += other.repaired;
        self.unchanged += other.unchanged;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }

    pub fn is_clean(&self) -> bool {
        self.failed == 0
    }
}

/// A single raw candidate row surfaced by the scan.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BackfillCandidate {
    pub event_id: i64,
    pub request_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    /// Upstream-resolved model label from `request_records.model`. Used as a
    /// fallback when `requested_model` is NULL so the billing refresh can
    /// still match a `billing_price_rules` row.
    pub model: Option<String>,
    pub existing_input_tokens: Option<i64>,
    pub existing_output_tokens: Option<i64>,
    pub existing_total_tokens: Option<i64>,
    pub existing_cached_tokens: Option<i64>,
    pub existing_cache_read_tokens: Option<i64>,
    pub existing_cache_write_tokens: Option<i64>,
    pub response_in_postgres: bool,
    pub request_in_postgres: bool,
    pub raw_object_only: bool,
    /// `true` when the upstream capture was truncated, meaning the retained
    /// raw body cannot represent the full usage. The backfill must refuse to
    /// overwrite tokens for such rows so a partial body does not erase the
    /// existing totals.
    pub response_capture_truncated: bool,
}

/// The decision the parser makes for one candidate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillDecision {
    /// Parsed usage differs from the stored row and will be written on apply.
    Repaired,
    /// Parsed usage already matches the stored row.
    Unchanged,
    /// Raw payload is missing or only available in object store. The row is
    /// never overwritten; the operator is expected to surface this to a
    /// follow-up tool that can fetch from object storage.
    Skipped,
    /// Raw payload was available but could not be parsed into a non-empty
    /// usage block, or the apply transaction failed. The row is never
    /// overwritten on a parse failure; an apply failure rolls back per-event
    /// so the surrounding batch keeps moving.
    Failed,
}

impl BackfillDecision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Repaired => "repaired",
            Self::Unchanged => "unchanged",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }

    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// Outcome of processing a single candidate. Returned regardless of `apply`.
#[derive(Debug, Clone)]
pub struct BackfillOutcome {
    pub event_id: i64,
    pub decision: BackfillDecision,
    pub reason: Option<String>,
}

/// Aggregated stats bucket labels, exposed for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsBucket {
    Repaired,
    Unchanged,
    Skipped,
    Failed,
}

/// Per-batch result returned by `backfill_token_usage`. `last_event_id` is
/// the largest `event_id` observed across the batch (or the cursor the batch
/// started with when no rows were scanned) so the caller can advance the
/// key-set cursor without re-reading rows.
///
/// `diagnostics` carries bounded per-row outcomes that the operator must see:
/// every `Failed` outcome plus a representative subset of `Skipped` outcomes
/// (truncated raw, raw missing / object-only, response body missing). The
/// `Repaired` and `Unchanged` outcomes are intentionally excluded so the
/// diagnostic stream stays small enough for `--apply` runs of any size. The
/// cursor advances past `Skipped` and `Failed` rows so a single bad event
/// never wedges the run.
#[derive(Debug, Clone)]
pub struct BackfillBatchOutcome {
    pub stats: BackfillStats,
    pub last_event_id: i64,
    pub diagnostics: Vec<BackfillOutcome>,
}

/// Returns true when the parsed usage differs from the stored row in any of
/// the canonical token columns.
pub fn decide_repair(candidate: &BackfillCandidate, parsed: &TokenUsage) -> bool {
    field_changed(candidate.existing_input_tokens, parsed.input_tokens)
        || field_changed(candidate.existing_output_tokens, parsed.output_tokens)
        || field_changed(candidate.existing_total_tokens, parsed.total_tokens)
        || field_changed(candidate.existing_cached_tokens, parsed.cached_tokens)
        || field_changed(
            candidate.existing_cache_read_tokens,
            parsed.cache_read_tokens,
        )
        || field_changed(
            candidate.existing_cache_write_tokens,
            parsed.cache_write_tokens,
        )
}

/// Returns true when the parsed usage differs from the stored row in a
/// *known* token column.
///
/// `(Some(_), None)` is treated as "parsed did not report a value for this
/// column" and is **not** considered a change — the unknown parsed field must
/// never erase a stored value. `(None, Some(_))` and unequal `Some` values
/// remain repairs. `Some(0)` is authoritative on either side.
fn field_changed(existing: Option<i64>, parsed: Option<i64>) -> bool {
    match (existing, parsed) {
        (Some(existing), Some(parsed)) => existing != parsed,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (None, None) => false,
    }
}

/// Pick the price-rule lookup key: `requested_model` wins, then `model`.
/// Mirrors `record_usage_charge` so the backfill cannot silently drift from
/// the new-request billing path.
pub fn billing_lookup_key(candidate: &BackfillCandidate) -> Option<String> {
    candidate
        .requested_model
        .clone()
        .or_else(|| candidate.model.clone())
}

/// Classifies an outcome for stats aggregation. Lives next to the enum so the
/// rule "apply-failed must count as failed; raw-missing and object-only stay
/// skipped" can be unit-tested without standing up a database.
pub fn classify_outcome(outcome: &BackfillOutcome) -> StatsBucket {
    match outcome.decision {
        BackfillDecision::Repaired => StatsBucket::Repaired,
        BackfillDecision::Unchanged => StatsBucket::Unchanged,
        BackfillDecision::Skipped => StatsBucket::Skipped,
        BackfillDecision::Failed => StatsBucket::Failed,
    }
}
