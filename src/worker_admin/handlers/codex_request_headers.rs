//! Issue #599 R2f.1 + #633: request-context headers for the Codex
//! subscription request.
//!
//! Split out of `chatgpt_backend.rs` to keep that mapping layer bounded. The
//! module knows only the two context headers the Codex backend accepts beyond
//! auth: the caller's stable `session-id` mirror and the access token's
//! compute-residency claim. The session value comes from the shared
//! `session_affinity` alias resolver, so conversation logging and upstream
//! forwarding never diverge. Nothing here is synthesized from per-request
//! state (`prompt_cache_key`, thread IDs, request IDs) and no value is
//! logged.

use reqwest::RequestBuilder;

use super::codex_claims;
use crate::session_affinity::{
    has_conflicting_session_affinity, resolve_session_affinity, session_affinity_source,
};

/// Request-path Codex headers beyond auth (issue #599 R2f.1, aliases per
/// issue #633): mirror the caller's stable session identifier as upstream
/// `session-id` and add the access token's compute-residency claim when it
/// carries a usable one. Both are presence-driven — absent or blank affinity
/// aliases and a missing/blank/`no_constraint` claim leave the header set
/// untouched, and nothing is synthesized from per-request state. Used only
/// for the subscription request path; the quota fetch keeps the plain
/// `chatgpt_backend::with_codex_headers`.
pub fn with_codex_request_headers(
    builder: RequestBuilder,
    access_token: &str,
    request_headers: &[(String, String)],
) -> RequestBuilder {
    let builder = match resolve_session_affinity(request_headers) {
        Some(session_id) => {
            if has_conflicting_session_affinity(request_headers) {
                // Allowlisted diagnostics only: which alias won, not the
                // values, so a conflict is visible without retaining secrets.
                tracing::warn!(
                    event = "codex_session_affinity_conflict",
                    source = session_affinity_source(request_headers).unwrap_or("unknown"),
                    "conflicting session-affinity aliases; forwarding the precedence winner"
                );
            }
            builder.header(CODEX_SESSION_HEADER, session_id)
        }
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
