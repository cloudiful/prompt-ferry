//! Phase 1 property tests for the upstream redaction session (issue #569).
//!
//! These lock the two executable definitions that the single-unit-test suite
//! missed before #564:
//!
//! * `p2` — with a prior session, one entity keeps the same token across turns.
//! * `p3` — an unknown-scope, mutated, or truncated token is never falsely
//!   restored: it survives verbatim and is counted as a diagnostic.

use std::collections::BTreeMap;

use prompt_ferry_redact::test_support::domain_redaction;
use proptest::prelude::*;
use redactor::InputKind;

use crate::{UpstreamRedactionSession, redact_text_with_stateful_session};

use super::token_for;

const TOKEN_PREFIX: &str = "[[RDX:v2:";
const TOKEN_PREFIX_LEN: usize = TOKEN_PREFIX.len();
const CHECKSUM_LEN: usize = 8;

const DOMAIN_NAMES: &[&str] = &[
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
];
const ROUND_PREFIXES: &[&str] = &["", "user: ", "tool: ", "again "];
const PREFIX_FILLERS: &[&str] = &["", "before ", "note: ", "line\n"];
const SUFFIX_FILLERS: &[&str] = &["", " after", "\nend", " trailing"];

fn domain(name: &str) -> String {
    format!("{name}.example.com")
}

fn one_session(entity: &str, external_id: &str) -> UpstreamRedactionSession {
    redact_text_with_stateful_session(entity, InputKind::Text, None, Some(external_id), None)
        .expect("redact")
        .session
        .expect("redaction minted no session")
}

fn p2_case() -> impl Strategy<Value = (Vec<String>, Vec<String>)> {
    (
        prop::sample::subsequence(DOMAIN_NAMES.to_vec(), 2..=4),
        prop::collection::vec(prop::sample::select(ROUND_PREFIXES), 2..=4),
    )
        .prop_map(|(names, prefixes)| {
            (
                names.into_iter().map(domain).collect(),
                prefixes.into_iter().map(str::to_string).collect(),
            )
        })
}

fn p3_case() -> impl Strategy<Value = (u8, u32, usize, usize)> {
    (
        0u8..3,
        0u32..=100,
        0usize..PREFIX_FILLERS.len(),
        0usize..SUFFIX_FILLERS.len(),
    )
}

/// A cut past the full marker prefix but before the closing `]]`, so the
/// candidate is still recognized as an unterminated token.
fn truncate_point(token_len: usize, ratio: u32) -> usize {
    let floor = TOKEN_PREFIX_LEN + 1;
    let span = token_len.saturating_sub(floor + 1);
    floor + (span * ratio as usize) / 100
}

/// Flip one checksum character, keeping the token length and marker shape.
fn mutate_checksum(token: &str) -> String {
    let index = token.len() - (CHECKSUM_LEN + 2);
    let mut bytes = token.as_bytes().to_vec();
    bytes[index] = if bytes[index] == b'a' { b'b' } else { b'a' };
    String::from_utf8(bytes).expect("token is ASCII")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn p2_prior_session_tokens_stay_stable_across_turns((entities, prefixes) in p2_case()) {
        let _guard = domain_redaction();
        let mut prior: Option<UpstreamRedactionSession> = None;
        let mut first_tokens: BTreeMap<String, String> = BTreeMap::new();
        let mut scope: Option<String> = None;
        let mut previous_entries = 0usize;

        for prefix in &prefixes {
            let text = format!("{prefix}{}", entities.join(" "));
            let turn = redact_text_with_stateful_session(
                &text,
                InputKind::Text,
                None,
                Some("conv-p2"),
                prior.as_ref(),
            )
            .expect("redact");
            prop_assert!(turn.applied, "no replacement minted for {text:?}");
            prop_assert!(turn.redacted_text.contains(TOKEN_PREFIX));

            let session = turn.session.expect("established session");
            let current_scope = session.request_session().scope_id.clone();
            if let Some(expected) = &scope {
                prop_assert_eq!(expected.as_str(), current_scope.as_str());
            } else {
                scope = Some(current_scope);
            }

            let entries = session.request_session().entries.len();
            prop_assert!(entries >= previous_entries, "session shrank across turns");
            previous_entries = entries;

            for entity in &entities {
                let token = token_for(&session, entity);
                if let Some(expected) = first_tokens.get(entity) {
                    prop_assert_eq!(expected, &token, "token for {} drifted", entity);
                } else {
                    first_tokens.insert(entity.clone(), token);
                }
            }
            prior = Some(session);
        }
    }

    #[test]
    fn p3_unknown_and_truncated_tokens_are_never_restored(
        (kind, ratio, prefix_index, suffix_index) in p3_case()
    ) {
        let _guard = domain_redaction();
        let session = one_session("alpha.example.com", "conv-p3-a");
        let token = token_for(&session, "alpha.example.com");
        let (candidate, forbidden) = match kind {
            0 => {
                let foreign = one_session("bravo.example.com", "conv-p3-b");
                prop_assume!(
                    session.request_session().scope_id != foreign.request_session().scope_id
                );
                (
                    token_for(&foreign, "bravo.example.com"),
                    "bravo.example.com",
                )
            }
            1 => {
                let cut = truncate_point(token.len(), ratio);
                (token[..cut].to_string(), "alpha.example.com")
            }
            _ => (mutate_checksum(&token), "alpha.example.com"),
        };
        let input = format!(
            "{}{candidate}{}",
            PREFIX_FILLERS[prefix_index], SUFFIX_FILLERS[suffix_index]
        );

        let restored = session
            .restore_state
            .restore_text(&input)
            .expect("restore");

        prop_assert_eq!(&restored.restored_text, &input);
        prop_assert!(
            !restored.restored_text.contains(forbidden),
            "falsely restored {forbidden}"
        );
        let diagnostics = restored.skipped_tokens.len()
            + restored.unresolved_tokens.len()
            + restored.validation_errors.len();
        prop_assert!(diagnostics >= 1, "no diagnostic for {candidate}");
    }
}
