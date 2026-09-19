use super::token_for;
use crate::{redact_text_with_stateful_session, restore_text};
use prompt_ferry_redact::test_support::domain_redaction;
use prompt_ferry_redact::{RedactionConfig, apply_config, policy_generation};
use redactor::RedactionRules;

#[test]
fn toggle_and_concurrency() {
    let _guard = domain_redaction();
    let generation = policy_generation();
    let first = redact_text_with_stateful_session(
        "a.example.com and b.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-toggle"),
        None,
    )
    .expect("redact")
    .session
    .expect("session");
    assert_eq!(first.policy_generation, generation);
    assert_eq!(first.request_session().entries.len(), 2);

    // Toggling the policy off and on again advances the generation, so the
    // session minted before the toggle must not seed the next turn: reusing it
    // would resurrect the old token counter.
    apply_config(&RedactionConfig {
        enabled: false,
        ..RedactionConfig::default()
    })
    .expect("disable");
    apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .expect("enable");
    assert!(policy_generation() > generation);

    let rebuilt = redact_text_with_stateful_session(
        "a.example.com and c.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-toggle"),
        Some(&first),
    )
    .expect("redact")
    .session
    .expect("rebuilt session");
    assert!(rebuilt.policy_generation > first.policy_generation);
    assert_eq!(rebuilt.request_session().entries.len(), 2);
    assert_ne!(
        token_for(&first, "a.example.com"),
        token_for(&rebuilt, "a.example.com")
    );

    // Concurrent turns within one generation both reuse the shared prior
    // mapping, so whichever upsert wins still restores the conversation.
    let branch_a = redact_text_with_stateful_session(
        "d.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-toggle"),
        Some(&rebuilt),
    )
    .expect("redact")
    .session
    .expect("branch a");
    let branch_b = redact_text_with_stateful_session(
        "e.example.com",
        redactor::InputKind::Text,
        None,
        Some("conv-toggle"),
        Some(&rebuilt),
    )
    .expect("redact")
    .session
    .expect("branch b");
    assert_eq!(branch_a.policy_generation, branch_b.policy_generation);
    assert_eq!(
        branch_a.request_session().scope_id,
        branch_b.request_session().scope_id
    );
    assert_eq!(
        token_for(&branch_a, "a.example.com"),
        token_for(&rebuilt, "a.example.com")
    );
    assert_eq!(
        token_for(&branch_b, "a.example.com"),
        token_for(&rebuilt, "a.example.com")
    );

    let restored = restore_text(
        &format!(
            "{} and {}",
            token_for(&branch_a, "a.example.com"),
            token_for(&branch_a, "d.example.com")
        ),
        &branch_a,
    )
    .expect("restore");
    assert_eq!(restored.restored_text, "a.example.com and d.example.com");
}
