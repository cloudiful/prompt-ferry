//! Mutation-survivor assertions for the persisted session metadata accessors
//! (#569 Phase 2): `policy_version`, `current_policy_version`, and
//! `prior_entry_count` must carry the real values their callers persist and
//! compare, not constants.

use prompt_ferry_redact::test_support::domain_redaction;
use prompt_ferry_redact::{RedactionConfig, apply_config, policy_generation};
use redactor::{InputKind, RedactionRules};

use crate::{
    UpstreamRedactionProcessor, UpstreamRedactionSession, redact_text_with_stateful_session,
};

use super::token_for;

/// Force a policy-generation bump so the observed version is a value no
/// constant replacement can accidentally reproduce (generation starts at 0
/// and a single identical re-apply keeps it).
fn bumped_generation() -> u64 {
    let generation = policy_generation();
    apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            secret: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .expect("apply distinct redaction config");
    let bumped = policy_generation();
    assert!(
        bumped > generation,
        "distinct config must bump the generation"
    );
    bumped
}

#[test]
fn session_policy_version_is_the_persisted_i64_generation() {
    let _guard = domain_redaction();
    let generation = bumped_generation();
    let session = redact_text_with_stateful_session(
        "a.example.com",
        InputKind::Text,
        None,
        Some("conv-meta"),
        None,
    )
    .expect("redact")
    .session
    .expect("session");

    assert_eq!(session.policy_generation, generation);
    assert_eq!(
        session.policy_version(),
        i64::try_from(generation).expect("generation fits i64")
    );
    assert_eq!(session.policy_version(), crate::current_policy_version());
    assert_ne!(session.policy_version(), 0);
}

#[test]
fn current_policy_version_tracks_the_running_generation() {
    let _guard = domain_redaction();
    let generation = bumped_generation();
    assert_eq!(
        crate::current_policy_version(),
        i64::try_from(generation).expect("generation fits i64")
    );
}

#[test]
fn prior_entry_count_counts_prior_session_entries() {
    let _guard = domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        InputKind::Text,
        None,
        Some("conv-meta"),
        None,
    )
    .expect("redact")
    .session
    .expect("first session");
    assert_eq!(first.request_session().entries.len(), 2);

    let mut processor =
        UpstreamRedactionProcessor::new(None, Some("conv-meta"), Some(&first)).expect("processor");
    assert_eq!(processor.prior_entry_count(), 2);

    // Mint one more token so the finalized state carries prior + new entries.
    let redacted = processor
        .redact_fragment("c.example.com", InputKind::Text)
        .expect("redact");
    let second = processor
        .finish_state("c.example.com", &redacted)
        .expect("finish")
        .expect("second session");
    assert_eq!(second.request_session().entries.len(), 3);
    assert_eq!(
        token_for(&second, "a.example.com"),
        token_for(&first, "a.example.com")
    );
}

#[test]
fn prior_entry_count_is_zero_without_a_prior_session() {
    let _guard = domain_redaction();
    let processor =
        UpstreamRedactionProcessor::new(None, Some("conv-meta-fresh"), None).expect("processor");
    assert_eq!(processor.prior_entry_count(), 0);
}

#[test]
fn current_constructor_stamps_the_running_generation() {
    let _guard = domain_redaction();
    let generation = bumped_generation();
    let redactor = prompt_ferry_redact::redactor_snapshot_for_user(None).expect("snapshot");
    let artifact = redactor
        .redact_artifact_with_input_kind_source_and_prior_session(
            "a.example.com",
            InputKind::Text,
            None,
            None,
            Some("conv-meta"),
        )
        .expect("redact");
    let session = UpstreamRedactionSession::current(
        redactor::RestoreState::new(artifact.session).expect("state"),
    );
    assert_eq!(session.policy_generation, generation);
    assert_eq!(
        session.policy_version(),
        i64::try_from(generation).expect("generation fits i64")
    );
}
