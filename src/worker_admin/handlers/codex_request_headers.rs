//! Issue #599 R2f.1: request-context headers for the Codex subscription
//! request.
//!
//! Split out of `chatgpt_backend.rs` to keep that mapping layer bounded. The
//! module knows only the two context headers the Codex backend accepts beyond
//! auth: the caller's stable `session-id` mirror and the access token's
//! compute-residency claim. Nothing here is synthesized from per-request state
//! and no value is logged.

use reqwest::RequestBuilder;

use super::codex_claims;

/// Request-path Codex headers beyond auth (issue #599 R2f.1): mirror the
/// caller's stable `session-id` and add the access token's compute-residency
/// claim when it carries a usable one. Both are presence-driven — an absent or
/// blank `session-id` and a missing/blank/`no_constraint` claim leave the
/// header set untouched, and nothing is synthesized from per-request state.
/// Used only for the subscription request path; the quota fetch keeps the
/// plain `chatgpt_backend::with_codex_headers`.
pub fn with_codex_request_headers(
    builder: RequestBuilder,
    access_token: &str,
    request_headers: &[(String, String)],
) -> RequestBuilder {
    let builder = match caller_session_id(request_headers) {
        Some(session_id) => builder.header(CODEX_SESSION_HEADER, session_id),
        None => builder,
    };
    match codex_claims::compute_residency_from_access_token(access_token) {
        Some(residency) => builder.header(CODEX_RESIDENCY_HEADER, residency),
        None => builder,
    }
}

/// Caller session header mirrored to the Codex backend. It identifies a
/// conversation session that outlives a single turn, so a per-turn request id
/// is never substituted for it.
const CODEX_SESSION_HEADER: &str = "session-id";
/// Compute-residency header, set only when the access token claims one.
const CODEX_RESIDENCY_HEADER: &str = "x-openai-internal-codex-residency";

/// Caller-supplied stable session identifier. The header name matches
/// case-insensitively and a blank value counts as absent.
fn caller_session_id(request_headers: &[(String, String)]) -> Option<&str> {
    request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(CODEX_SESSION_HEADER))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}
