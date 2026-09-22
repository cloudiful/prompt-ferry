use super::{UpstreamRedactionSession, redact_text_with_stateful_session, restore_text};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use prompt_ferry_redact::test_support::domain_redaction;
use prompt_ferry_runtime_env::relay_secrets::RelaySecretManager;

mod toggle;

fn token_for(session: &UpstreamRedactionSession, original: &str) -> String {
    session
        .request_session()
        .entries
        .iter()
        .find(|entry| entry.original == original)
        .map(|entry| entry.token.clone())
        .expect("token")
}

#[test]
fn session_budget_overflow_without_prior_downgrades_to_none() {
    let _guard = domain_redaction();
    let text = (0..super::MAX_ENTRIES + 100)
        .map(|index| format!("budget{index}.example.com"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = redact_text_with_stateful_session(
        &text,
        redactor::InputKind::Text,
        None,
        Some("conv-budget"),
        None,
    )
    .expect("redact");

    assert!(result.redacted_text.contains("[[RDX:v2:"));
    assert!(result.applied);
    assert!(result.session.is_none());
}

#[test]
fn session_budget_overflow_with_prior_keeps_the_prior_state() {
    let _guard = domain_redaction();
    let prior_text = (0..super::MAX_ENTRIES)
        .map(|index| format!("prior{index}.example.com"))
        .collect::<Vec<_>>()
        .join(" ");
    let prior = redact_text_with_stateful_session(
        &prior_text,
        redactor::InputKind::Text,
        None,
        Some("conv-prior-budget"),
        None,
    )
    .expect("prior redact")
    .session
    .expect("prior session");
    assert_eq!(prior.request_session().entries.len(), super::MAX_ENTRIES);

    let overflow = redact_text_with_stateful_session(
        "prior0.example.com and overflow.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-prior-budget"),
        Some(&prior),
    )
    .expect("overflow redact");

    assert!(overflow.applied);
    let kept = overflow
        .session
        .expect("a refused turn must keep the prior state");
    assert_eq!(kept, prior, "the kept state must be the prior state");
    assert_eq!(kept.request_session().entries.len(), super::MAX_ENTRIES);
    assert_eq!(
        token_for(&kept, "prior0.example.com"),
        token_for(&prior, "prior0.example.com"),
        "existing entities keep their token after a refused turn"
    );
    assert!(
        kept.request_session()
            .entries
            .iter()
            .all(|entry| entry.original != "overflow.example.com"),
        "a refused turn must not persist its new mapping"
    );
}

#[test]
fn permit_ceiling_overflow_with_prior_keeps_the_prior_state() {
    let _guard = domain_redaction();
    let prior = permit_capped_prior();
    assert_eq!(prior.restore_state.permits().len(), super::MAX_PERMITS);
    let first_token = token_for(&prior, "permit0.example.com");

    let overflow = redact_text_with_stateful_session(
        "permit0.example.com and next.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-permit-budget"),
        Some(&prior),
    )
    .expect("overflow redact");

    assert!(overflow.applied);
    let kept = overflow
        .session
        .expect("the permit ceiling must keep the prior state");
    assert_eq!(kept, prior);
    assert_eq!(token_for(&kept, "permit0.example.com"), first_token);
    assert!(
        kept.request_session()
            .entries
            .iter()
            .all(|entry| entry.original != "next.example.com"),
        "a refused turn must not persist its new mapping"
    );
}

/// Stream 500 turn-advances through the raw session API (the same advance chain
/// `UpstreamRedactionProcessor` uses) until the state sits exactly on the
/// permit ceiling.
fn permit_capped_prior() -> UpstreamRedactionSession {
    let redactor = prompt_ferry_redact::redactor_snapshot_for_user(None).expect("redactor");
    let mut session_redactor =
        redactor::SessionRedactor::with_prior_session(None, Some("conv-permit-budget"))
            .expect("session redactor");
    let mut state: Option<redactor::RestoreState> = None;
    for turn in 0..super::MAX_PERMITS {
        let text = format!("permit{turn}.example.com");
        let redacted = session_redactor
            .redact_fragment_with_input_kind(&redactor, &text, redactor::InputKind::Text)
            .expect("advance redact");
        let session = session_redactor.finish_session(&text, &redacted, redactor.policy());
        state = Some(
            match &state {
                Some(prior) => prior.advance(session),
                None => redactor::RestoreState::new(session),
            }
            .expect("advance state"),
        );
    }
    UpstreamRedactionSession::current(state.expect("permit-capped state"))
}

#[test]
fn large_session_is_persisted_and_keeps_tokens_across_turns() {
    let _guard = domain_redaction();
    let filler = "lorem ipsum dolor sit amet ".repeat(20_000);
    let text = format!("{filler} a.example.com");
    let first = redact_text_with_stateful_session(
        &text,
        redactor::InputKind::Text,
        None,
        Some("conv-large-budget"),
        None,
    )
    .expect("redact");

    assert!(first.applied);
    let first_session = first
        .session
        .expect("a large conversation must still persist its session");
    let serialized_len = serde_json::to_vec(&first_session)
        .expect("serialize session")
        .len();
    assert!(
        serialized_len > 512 * 1024,
        "test session must be large: {serialized_len} bytes"
    );
    assert!(serialized_len < super::MAX_BYTES);
    let first_token = token_for(&first_session, "a.example.com");
    assert!(!first_token.is_empty());

    let second = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-large-budget"),
        Some(&first_session),
    )
    .expect("second redact");
    let second_session = second.session.expect("second session");

    assert_eq!(
        token_for(&second_session, "a.example.com"),
        first_token,
        "replaying a large conversation must keep its existing token"
    );
    assert_eq!(second_session.request_session().entries.len(), 2);
}

#[test]
fn stateful_tokens_reused_across_turns() {
    let _guard = domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact");
    let first_session = first.session.expect("session");
    let second = redact_text_with_stateful_session(
        "b.example.com then c.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first_session),
    )
    .expect("redact");
    let second_session = second.session.expect("session");

    assert!(first.redacted_text.contains("[[RDX:v2:"));
    assert!(second.redacted_text.contains("[[RDX:v2:"));
    assert_eq!(
        second_session.request_session().scope_id,
        first_session.request_session().scope_id
    );
    assert_eq!(
        second_session.request_session().external_id.as_deref(),
        Some("conv-1")
    );
    assert_eq!(
        token_for(&first_session, "b.example.com"),
        token_for(&second_session, "b.example.com")
    );
    assert_eq!(second_session.request_session().entries.len(), 3);
}

#[test]
fn unauthorized_token_is_preserved_and_reported() {
    let _guard = domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact")
    .session
    .expect("first");
    let second = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    )
    .expect("redact")
    .session
    .expect("second");

    let second_text = &second.request_session().redacted_text;
    let restored = restore_text(second_text, &first).expect("restore");
    assert!(restored.is_valid());
    assert!(restored.restored_text.starts_with("a.example.com and "));
    assert_eq!(restored.skipped_tokens.len(), 1);
    assert!(
        restored
            .restored_text
            .ends_with(&restored.skipped_tokens[0])
    );
}

#[test]
fn encrypted_session_round_trip() {
    let _guard = domain_redaction();
    let key = STANDARD.encode([7_u8; 32]);
    let manager = RelaySecretManager::from_base64(&key).expect("mgr");
    let session = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact")
    .session
    .expect("session");
    let encrypted = super::encrypt_upstream_session(&manager, &session).expect("encrypt");
    let decrypted = super::decrypt_upstream_session(&manager, &encrypted).expect("decrypt");
    assert_eq!(decrypted, session);
}

#[test]
fn legacy_session_envelope_is_rejected() {
    let _guard = domain_redaction();
    let key = STANDARD.encode([7_u8; 32]);
    let manager = RelaySecretManager::from_base64(&key).expect("mgr");
    let session = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact")
    .session
    .expect("session");
    let legacy = serde_json::json!({"request_session": session.request_session()});
    let envelope = manager
        .encrypt(&legacy.to_string())
        .expect("encrypt legacy");

    assert!(super::decrypt_upstream_session(&manager, &envelope).is_err());
}

#[test]
fn later_session_restores_earlier_and_new_tokens() {
    let _guard = domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact")
    .session
    .expect("first");
    let second = redact_text_with_stateful_session(
        "b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    )
    .expect("redact")
    .session
    .expect("second");

    let restored = restore_text(
        &format!(
            "{} and {}",
            token_for(&first, "a.example.com"),
            token_for(&second, "b.example.com")
        ),
        &second,
    )
    .expect("restore");
    assert!(restored.is_valid());
    assert_eq!(restored.restored_text, "a.example.com and b.example.com");
}

#[test]
fn prior_state_survives_turn_without_new_replacements() {
    let _guard = domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .expect("redact")
    .session
    .expect("first");
    let token = token_for(&first, "a.example.com");
    let second = redact_text_with_stateful_session(
        "continue",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    )
    .expect("redact");

    assert!(!second.applied);
    let second = second.session.expect("retained state");
    let restored = restore_text(&token, &second).expect("restore");
    assert_eq!(restored.restored_text, "a.example.com");
}
