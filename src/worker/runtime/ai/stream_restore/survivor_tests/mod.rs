//! Mutation-survivor assertions for the SSE restore filter (#569 Phase 2).
//!
//! Split by responsibility: chunk batching and terminal lifecycle
//! (`chunking`), the Responses error-body capture (`error_terminal`),
//! event-family restore arms and SSE framing (`event_families`), and the
//! pure helper functions (`scanners`). Each test pins one observable behavior
//! the mutation batch showed to be under-asserted; all are fast and
//! deterministic so a surviving mutant fails within the lib suite run.

mod chunking;
mod error_terminal;
mod event_families;
mod scanners;

use redactor::{FindingKind, InputKind, RedactionPolicy, RedactorBuilder, RestoreState};

use super::SseRestoreFilter;
use crate::redact_upstream::UpstreamRedactionSession;

/// A session with one redacted domain, plus its issued token.
pub(super) fn session(original: &str) -> (UpstreamRedactionSession, String) {
    let redactor = RedactorBuilder::new()
        .with_redaction_policy(RedactionPolicy::default().with_kind(FindingKind::Domain, true))
        .build();
    let artifact = redactor
        .redact_artifact_with_input_kind_source_and_prior_session(
            original,
            InputKind::Text,
            None,
            None,
            Some("conversation"),
        )
        .expect("redact");
    let token = artifact.session.issued_tokens[0].clone();
    (
        UpstreamRedactionSession::current(RestoreState::new(artifact.session).expect("state")),
        token,
    )
}

/// Parse the payload of the first `data: ` line of an SSE event as JSON.
pub(super) fn data_json(event: &[u8]) -> serde_json::Value {
    let line = std::str::from_utf8(event)
        .expect("UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    serde_json::from_str(line).expect("JSON")
}

/// A Responses output-text delta event carrying `delta` as its payload.
pub(super) fn delta_event(item_id: &str, output_index: u64, delta: &str) -> String {
    format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"{item_id}\",\"output_index\":{output_index},\"content_index\":0,\"delta\":{delta:?}}}\n\n"
    )
}
