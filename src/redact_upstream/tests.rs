use super::{UpstreamRedactionSession, redact_text_with_stateful_session, restore_text};
use crate::redact::{RedactionConfig, apply_config};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use redactor::RedactionRules;

fn token_for(session: &UpstreamRedactionSession, original: &str) -> String {
    session
        .request_session()
        .entries
        .iter()
        .find(|entry| entry.original == original)
        .map(|entry| entry.token.clone())
        .expect("token")
}

fn enable_domain_redaction() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::redact::TEST_REDACTION_LOCK.lock().expect("lock");
    apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .expect("config");
    guard
}

#[test]
fn stateful_tokens_reused_across_turns() {
    let _guard = enable_domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    );
    let first_session = first.session.expect("session");
    let second = redact_text_with_stateful_session(
        "b.example.com then c.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first_session),
    );
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
    let _guard = enable_domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .session
    .expect("first");
    let second = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    )
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
    let _guard = enable_domain_redaction();
    let key = STANDARD.encode([7_u8; 32]);
    let manager = crate::relay_secrets::RelaySecretManager::from_base64(&key).expect("mgr");
    let session = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .session
    .expect("session");
    let encrypted = super::encrypt_upstream_session(&manager, &session).expect("encrypt");
    let decrypted = super::decrypt_upstream_session(&manager, &encrypted).expect("decrypt");
    assert_eq!(decrypted, session);
}

#[test]
fn legacy_session_envelope_is_rejected() {
    let _guard = enable_domain_redaction();
    let key = STANDARD.encode([7_u8; 32]);
    let manager = crate::relay_secrets::RelaySecretManager::from_base64(&key).expect("mgr");
    let session = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
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
    let _guard = enable_domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .session
    .expect("first");
    let second = redact_text_with_stateful_session(
        "b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    )
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
    let _guard = enable_domain_redaction();
    let first = redact_text_with_stateful_session(
        "a.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        None,
    )
    .session
    .expect("first");
    let token = token_for(&first, "a.example.com");
    let second = redact_text_with_stateful_session(
        "continue",
        redactor::InputKind::Text,
        None,
        Some("conv-1"),
        Some(&first),
    );

    assert!(!second.applied);
    let second = second.session.expect("retained state");
    let restored = restore_text(&token, &second).expect("restore");
    assert_eq!(restored.restored_text, "a.example.com");
}
