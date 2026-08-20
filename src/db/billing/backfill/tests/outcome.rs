//! Tests for the `classify_outcome` helper and the BackfillStats aggregator.

use crate::db::billing::backfill::{
    BackfillDecision, BackfillOutcome, BackfillStats, StatsBucket, classify_outcome,
};

#[test]
fn classify_outcome_treats_apply_failure_as_failed_not_skipped() {
    let outcome = BackfillOutcome {
        event_id: 42,
        decision: BackfillDecision::Failed,
        reason: Some("apply-failed: connection reset".to_string()),
    };
    assert!(outcome.decision.is_failure());
    assert_eq!(classify_outcome(&outcome), StatsBucket::Failed);
}

#[test]
fn classify_outcome_treats_parse_failure_as_failed() {
    let outcome = BackfillOutcome {
        event_id: 7,
        decision: BackfillDecision::Failed,
        reason: Some("parse-failed: no usage block recovered".to_string()),
    };
    assert_eq!(classify_outcome(&outcome), StatsBucket::Failed);
}

#[test]
fn classify_outcome_treats_load_failure_as_failed() {
    let outcome = BackfillOutcome {
        event_id: 11,
        decision: BackfillDecision::Failed,
        reason: Some("load-failed: connection reset".to_string()),
    };
    assert_eq!(classify_outcome(&outcome), StatsBucket::Failed);
}

#[test]
fn classify_outcome_treats_raw_missing_as_skipped() {
    let outcome = BackfillOutcome {
        event_id: 11,
        decision: BackfillDecision::Skipped,
        reason: Some("raw payload unavailable".to_string()),
    };
    assert_eq!(classify_outcome(&outcome), StatsBucket::Skipped);
}

#[test]
fn classify_outcome_treats_truncated_as_skipped() {
    let outcome = BackfillOutcome {
        event_id: 13,
        decision: BackfillDecision::Skipped,
        reason: Some(crate::db::billing::backfill::SKIPPED_TRUNCATED_REASON.to_string()),
    };
    assert_eq!(classify_outcome(&outcome), StatsBucket::Skipped);
}

#[test]
fn classify_outcome_treats_repaired_and_unchanged_correctly() {
    let repaired = BackfillOutcome {
        event_id: 1,
        decision: BackfillDecision::Repaired,
        reason: None,
    };
    let unchanged = BackfillOutcome {
        event_id: 2,
        decision: BackfillDecision::Unchanged,
        reason: None,
    };
    assert_eq!(classify_outcome(&repaired), StatsBucket::Repaired);
    assert_eq!(classify_outcome(&unchanged), StatsBucket::Unchanged);
}

#[test]
fn backfill_stats_is_clean_only_when_no_failures() {
    let mut stats = BackfillStats::default();
    assert!(stats.is_clean());
    stats.failed = 1;
    assert!(!stats.is_clean());
    stats.failed = 0;
    stats.skipped = 5;
    assert!(stats.is_clean());
}

#[test]
fn backfill_stats_add_aggregates_fields() {
    let mut a = BackfillStats::default();
    a.scanned = 10;
    a.repaired = 2;
    a.unchanged = 3;
    a.skipped = 4;
    a.failed = 1;
    let mut b = BackfillStats::default();
    b.scanned = 5;
    b.repaired = 1;
    b.unchanged = 1;
    b.skipped = 2;
    b.failed = 1;
    a.add(b);
    assert_eq!(a.scanned, 15);
    assert_eq!(a.repaired, 3);
    assert_eq!(a.unchanged, 4);
    assert_eq!(a.skipped, 6);
    assert_eq!(a.failed, 2);
}
