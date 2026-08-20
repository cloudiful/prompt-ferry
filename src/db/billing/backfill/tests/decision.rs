//! Tests for the `decide_repair` and `billing_lookup_key` helpers.

use crate::db::billing::backfill::{billing_lookup_key, decide_repair};
use crate::usage::TokenUsage;

use super::fixtures::{candidate, candidate_with_models};

#[test]
fn decision_marks_existing_correct_row_as_unchanged() {
    let candidate = candidate(1, Some(82976), Some(82793), Some(7), true, false, false);
    let parsed = TokenUsage {
        input_tokens: Some(82976),
        output_tokens: Some(42),
        total_tokens: Some(83018),
        cached_tokens: Some(82793),
        cache_read_tokens: Some(82793),
        cache_write_tokens: Some(7),
    };
    assert!(!decide_repair(&candidate, &parsed));
}

#[test]
fn decision_marks_old_anthropic_split_input_as_repaired() {
    let candidate = candidate(712_098, Some(176), Some(82793), Some(7), true, false, false);
    let parsed = TokenUsage {
        input_tokens: Some(82976),
        output_tokens: Some(42),
        total_tokens: Some(83018),
        cached_tokens: Some(82793),
        cache_read_tokens: Some(82793),
        cache_write_tokens: Some(7),
    };
    assert!(decide_repair(&candidate, &parsed));
}

#[test]
fn decision_treats_object_only_payload_as_skipped() {
    let candidate = candidate(7, Some(120), Some(30), Some(7), false, true, false);
    let parsed = TokenUsage {
        input_tokens: Some(157),
        output_tokens: Some(20),
        total_tokens: Some(177),
        cached_tokens: Some(30),
        cache_read_tokens: Some(30),
        cache_write_tokens: Some(7),
    };
    assert!(candidate.raw_object_only);
    assert!(decide_repair(&candidate, &parsed));
}

#[test]
fn decision_treats_truncated_candidate_as_no_repair_signal() {
    let candidate = candidate(9, Some(120), Some(30), Some(7), true, false, true);
    let parsed = TokenUsage {
        input_tokens: Some(157),
        output_tokens: Some(20),
        total_tokens: Some(177),
        cached_tokens: Some(30),
        cache_read_tokens: Some(30),
        cache_write_tokens: Some(7),
    };
    assert!(candidate.response_capture_truncated);
    assert!(decide_repair(&candidate, &parsed));
}

#[test]
fn decide_repair_treats_parsed_none_for_existing_some_as_no_change() {
    let candidate = candidate(1, Some(82976), Some(82793), Some(7), true, false, false);
    let parsed_partial = TokenUsage {
        input_tokens: None,
        output_tokens: Some(42),
        total_tokens: None,
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    assert!(!decide_repair(&candidate, &parsed_partial));
}

#[test]
fn decide_repair_treats_parsed_zero_as_authoritative() {
    let candidate = candidate(1, Some(82976), Some(82793), Some(7), true, false, false);
    let parsed_zero = TokenUsage {
        input_tokens: Some(0),
        output_tokens: Some(0),
        total_tokens: Some(0),
        cached_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
    };
    assert!(decide_repair(&candidate, &parsed_zero));
}

#[test]
fn decide_repair_treats_existing_none_parsed_some_as_change() {
    let candidate = candidate(1, None, None, None, true, false, false);
    let parsed = TokenUsage {
        input_tokens: Some(120),
        output_tokens: Some(20),
        total_tokens: Some(140),
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    assert!(decide_repair(&candidate, &parsed));
}

#[test]
fn field_changed_table_covers_all_four_cases() {
    use super::fixtures::{equal_some_candidate, none_candidate, stored_some_candidate};

    let equal_some = equal_some_candidate(1);
    let parsed_equal = TokenUsage {
        input_tokens: Some(7),
        output_tokens: Some(7),
        total_tokens: Some(7),
        cached_tokens: Some(7),
        cache_read_tokens: Some(7),
        cache_write_tokens: Some(7),
    };
    assert!(!decide_repair(&equal_some, &parsed_equal));
    let parsed_off = TokenUsage {
        cache_read_tokens: Some(8),
        ..parsed_equal.clone()
    };
    assert!(decide_repair(&equal_some, &parsed_off));

    let both_none = none_candidate(2);
    let parsed_none = TokenUsage::default();
    assert!(!decide_repair(&both_none, &parsed_none));

    let stored_some = stored_some_candidate(3);
    let parsed_partial = TokenUsage {
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    assert!(!decide_repair(&stored_some, &parsed_partial));

    let stored_none = none_candidate(4);
    let parsed_some = TokenUsage {
        input_tokens: Some(120),
        output_tokens: None,
        total_tokens: None,
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    assert!(decide_repair(&stored_none, &parsed_some));
}

#[test]
fn billing_lookup_key_prefers_requested_model_over_model() {
    let candidate = candidate_with_models(1, Some("public-v1"), Some("public-v2"));
    assert_eq!(billing_lookup_key(&candidate).as_deref(), Some("public-v1"));
}

#[test]
fn billing_lookup_key_falls_back_to_model_when_requested_missing() {
    let candidate = candidate_with_models(2, None, Some("public-fallback"));
    assert_eq!(
        billing_lookup_key(&candidate).as_deref(),
        Some("public-fallback")
    );
}

#[test]
fn billing_lookup_key_returns_none_when_both_missing() {
    let candidate = candidate_with_models(3, None, None);
    assert_eq!(billing_lookup_key(&candidate), None);
}
