//! Issue #633 + #701: shared OpenCode session-affinity alias resolution.
//!
//! Both conversation logging and the Codex OAuth egress resolve the same
//! caller headers to one stable identifier.
//!
//! `x-opencode-session-id` (OpenCode 2.0.20+) carries the session that owns
//! the request, so it wins outright. The remaining aliases stay the fallback
//! for older OpenCode builds and other clients. OpenCode V2 sends those legacy
//! aliases with the parent session id next to its own header, so a differing
//! legacy value is expected rather than a conflict about the current identity.
//! Without that header a genuine disagreement is still reported, and either
//! way resolution stays deterministic (precedence wins) so logging and upstream
//! never route by different identities. Blank values are ignored, and nothing
//! ever derives from `prompt_cache_key`, thread IDs, or per-request IDs.
//!
//! Issue #1052 P1: the same module owns the upstream OpenCode request header
//! adapter. The OpenCode gateway rejects a request without a non-empty routing
//! session, so every direct upstream request to an OpenCode endpoint — the
//! inference path, the model catalog reads, the target probes, the client
//! model listing, and the usage query — carries the header through one shared
//! helper instead of a private runtime implementation.

use crate::db::EndpointProvider;
use http::header;
use uuid::Uuid;

/// OpenCode 2.0.20+ header naming the session that owns the request.
pub const OPENCODE_SESSION_HEADER: &str = "x-opencode-session-id";

/// Upstream header carrying the OpenCode routing session.
pub const OPENCODE_UPSTREAM_SESSION_HEADER: &str = "x-opencode-session";

/// Canonical alias precedence for the stable session identifier.
pub const SESSION_AFFINITY_ALIASES: [&str; 5] = [
    "x-opencode-session-id",
    "x-session-id",
    "x-session-affinity",
    "x-opencode-session",
    "session-id",
];

/// Parent-session metadata aliases, most specific first. Association and
/// display only; never a candidate for the current session identity.
pub const PARENT_SESSION_ALIASES: [&str; 2] =
    ["x-opencode-parent-session-id", "x-parent-session-id"];

fn alias_value<'a>(headers: &'a [(String, String)], alias: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(alias))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

/// Deterministic stable session identifier, or `None` when no alias carries
/// a non-empty value. Never derives from `prompt_cache_key`, thread IDs, or
/// per-request IDs.
pub fn resolve_session_affinity(headers: &[(String, String)]) -> Option<String> {
    SESSION_AFFINITY_ALIASES
        .iter()
        .find_map(|alias| alias_value(headers, alias))
        .map(str::to_string)
}

/// Which alias supplied [`resolve_session_affinity`], if any. The header name
/// is allowlisted diagnostics; the value itself is never exposed here.
pub fn session_affinity_source(headers: &[(String, String)]) -> Option<&'static str> {
    SESSION_AFFINITY_ALIASES
        .iter()
        .find(|alias| alias_value(headers, alias).is_some())
        .copied()
}

/// Parent-session metadata identifier, or `None` when no parent alias carries a
/// non-empty value. Association and display metadata only: conversation
/// grouping and upstream forwarding never read it.
pub fn resolve_parent_session_affinity(headers: &[(String, String)]) -> Option<String> {
    PARENT_SESSION_ALIASES
        .iter()
        .find_map(|alias| alias_value(headers, alias))
        .map(str::to_string)
}

/// True when two or more aliases carry distinct non-empty values. Callers
/// keep the precedence winner and must not silently diverge from it.
///
/// An OpenCode V2 current-session header settles the identity on its own: the
/// lineage-root aliases beside it are expected to hold the parent session, so
/// they are not reported as a disagreement about the current identity.
pub fn has_conflicting_session_affinity(headers: &[(String, String)]) -> bool {
    if alias_value(headers, OPENCODE_SESSION_HEADER).is_some() {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    for (name, value) in headers {
        if !SESSION_AFFINITY_ALIASES
            .iter()
            .any(|alias| name.eq_ignore_ascii_case(alias))
        {
            continue;
        }
        let trimmed = value.trim();
        if trimmed.is_empty() || seen.contains(&trimmed) {
            continue;
        }
        seen.push(trimmed);
        if seen.len() > 1 {
            return true;
        }
    }
    false
}

/// Exact OpenCode host detection.
///
/// The OpenCode host is the root domain or one of its subdomains, so a
/// lookalike domain (`notopencode.ai`, `opencode.ai.evil.com`), a deceptive
/// userinfo host (`https://opencode.ai@evil.test`), and a query or path that
/// merely mentions the domain never match.
pub fn is_opencode_host(url: &str) -> bool {
    let host = if let Ok(parsed) = reqwest::Url::parse(url) {
        parsed.host_str().map(|h| h.to_ascii_lowercase())
    } else {
        // Fallback manual parsing for robustness in tests with non-standard urls.
        let trimmed = url.trim();
        let without_scheme = if let Some(idx) = trimmed.find("://") {
            &trimmed[idx + 3..]
        } else {
            trimmed
        };
        let host_part = without_scheme
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        if host_part.is_empty() {
            None
        } else {
            Some(host_part.to_ascii_lowercase())
        }
    };
    match host {
        Some(host) => host == "opencode.ai" || host.ends_with(".opencode.ai"),
        None => false,
    }
}

/// Whether the OpenCode upstream headers apply to a request.
///
/// The gate is the OpenCode provider or an exact OpenCode host, so a generic
/// provider on an unrelated host never receives a synthetic OpenCode session.
pub fn opencode_headers_apply(provider: Option<EndpointProvider>, url: &str) -> bool {
    provider == Some(EndpointProvider::OpencodeGo) || is_opencode_host(url)
}

/// The OpenCode routing session for one upstream request.
///
/// Precedence: the caller's own `x-opencode-session`, then the shared session
/// aliases, then the stored conversation, then the request's own id. Blank
/// values are absent, so a stateless caller still gets a deterministic session.
pub fn resolve_opencode_session(
    request_headers: &[(String, String)],
    conversation_id: Option<Uuid>,
    request_id: Uuid,
) -> String {
    request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(OPENCODE_UPSTREAM_SESSION_HEADER))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| resolve_session_affinity(request_headers))
        .or_else(|| {
            conversation_id
                .filter(|id| !id.is_nil())
                .map(|id| format!("ses_{id}"))
        })
        .unwrap_or_else(|| format!("ses_{request_id}"))
}

/// Apply the OpenCode upstream headers to a request.
///
/// The request carries the routing session, the caller's `User-Agent` (or a
/// `prompt-ferry/<version>` default), and the other safe `x-opencode-*`
/// identity headers. Authorization, cookies, and hop-by-hop headers are never
/// forwarded, and no header value is logged.
pub fn apply_opencode_headers(
    builder: reqwest::RequestBuilder,
    request_headers: &[(String, String)],
    session: &str,
) -> reqwest::RequestBuilder {
    let builder = builder.header(OPENCODE_UPSTREAM_SESSION_HEADER, session);

    // Preserve caller User-Agent, else synthesize prompt-ferry/<version>.
    let user_agent_value = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let builder = if let Some(value) = user_agent_value {
        builder.header(header::USER_AGENT, value)
    } else {
        builder.header(
            header::USER_AGENT,
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION")),
        )
    };

    // Forward other safe OpenCode identity headers (x-opencode-*) that are non-empty.
    // This keeps the relay's safe header policy for OpenCode without leaking auth/hop-by-hop.
    let mut builder = builder;
    for (name, value) in request_headers {
        if name.eq_ignore_ascii_case(OPENCODE_UPSTREAM_SESSION_HEADER)
            || name.eq_ignore_ascii_case("user-agent")
        {
            continue;
        }
        if name.to_ascii_lowercase().starts_with("x-opencode-") {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Avoid forwarding auth/hop-by-hop even if mis-prefixed; x-opencode-* is safe, but keep guard.
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "cookie" | "x-api-key" | "host" | "connection"
            ) {
                continue;
            }
            builder = builder.header(name.as_str(), trimmed);
        }
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn each_alias_resolves_alone() {
        for alias in SESSION_AFFINITY_ALIASES {
            let resolved = resolve_session_affinity(&headers(&[(alias, "ses_1")]));
            assert_eq!(resolved.as_deref(), Some("ses_1"));
            assert_eq!(
                session_affinity_source(&headers(&[(alias, "ses_1")])),
                Some(alias)
            );
        }
    }

    #[test]
    fn opencode_v2_current_session_header_wins_over_lineage_root_aliases() {
        let request = headers(&[
            ("x-opencode-session-id", "ses_child"),
            ("x-opencode-parent-session-id", "ses_parent"),
            ("x-session-affinity", "ses_parent"),
            ("X-Session-Id", "ses_parent"),
            ("x-opencode-session", "ses_parent"),
            ("x-parent-session-id", "ses_parent"),
        ]);
        assert_eq!(
            resolve_session_affinity(&request).as_deref(),
            Some("ses_child"),
            "the child session owns the request even though the legacy aliases hold the parent"
        );
        assert_eq!(
            session_affinity_source(&request),
            Some(OPENCODE_SESSION_HEADER)
        );
        assert_eq!(
            resolve_parent_session_affinity(&request).as_deref(),
            Some("ses_parent")
        );
        assert!(
            !has_conflicting_session_affinity(&request),
            "a parent-valued lineage alias is not a conflict about the current identity"
        );
    }

    #[test]
    fn precedence_prefers_explicit_session_id() {
        let request = headers(&[
            ("x-session-affinity", "affinity"),
            ("x-opencode-session", "opencode"),
            ("session-id", "legacy"),
            ("x-session-id", "explicit"),
        ]);
        assert_eq!(
            resolve_session_affinity(&request).as_deref(),
            Some("explicit")
        );
        assert_eq!(session_affinity_source(&request), Some("x-session-id"));
    }

    #[test]
    fn blank_aliases_fall_through() {
        let request = headers(&[
            ("x-opencode-session-id", "   "),
            ("x-session-id", "   "),
            ("x-session-affinity", ""),
            ("x-opencode-session", "ses_ok"),
            ("session-id", "   "),
        ]);
        assert_eq!(
            resolve_session_affinity(&request).as_deref(),
            Some("ses_ok")
        );
        assert!(!has_conflicting_session_affinity(&request));
    }

    #[test]
    fn conflicting_aliases_are_detected_but_deterministic() {
        let request = headers(&[("x-session-id", "ses_a"), ("x-session-affinity", "ses_b")]);
        assert!(has_conflicting_session_affinity(&request));
        assert_eq!(resolve_session_affinity(&request).as_deref(), Some("ses_a"));
    }

    #[test]
    fn parent_alias_prefers_opencode_then_legacy_and_ignores_blanks() {
        let opencode = headers(&[
            ("x-parent-session-id", "ses_legacy_parent"),
            ("X-OpenCode-Parent-Session-Id", " ses_parent "),
        ]);
        assert_eq!(
            resolve_parent_session_affinity(&opencode).as_deref(),
            Some("ses_parent")
        );
        assert_eq!(
            resolve_parent_session_affinity(&headers(&[(
                "x-parent-session-id",
                "ses_legacy_parent"
            )]))
            .as_deref(),
            Some("ses_legacy_parent"),
            "the legacy parent alias stays a supported fallback"
        );
        assert_eq!(
            resolve_parent_session_affinity(&headers(&[("x-parent-session-id", "  ")])),
            None
        );
        assert_eq!(resolve_parent_session_affinity(&[]), None);
    }

    #[test]
    fn unrelated_headers_are_ignored() {
        let request = headers(&[
            ("request-id", "req_1"),
            ("prompt_cache_key", "cache_1"),
            ("x-opencode-project", "proj_1"),
            ("x-opencode-parent-session-id", "ses_parent"),
            ("authorization", "Bearer secret"),
        ]);
        assert_eq!(resolve_session_affinity(&request), None);
        assert_eq!(session_affinity_source(&request), None);
        assert!(!has_conflicting_session_affinity(&request));
    }

    #[test]
    fn names_match_case_insensitively_and_values_trim() {
        let request = headers(&[("X-Session-Affinity", "  ses_1  ")]);
        assert_eq!(resolve_session_affinity(&request).as_deref(), Some("ses_1"));
    }

    #[test]
    fn the_opencode_host_gate_accepts_the_root_and_its_subdomains_only() {
        assert!(is_opencode_host("https://opencode.ai"));
        assert!(is_opencode_host("https://api.opencode.ai/zen/go/v1"));
        assert!(is_opencode_host(
            "https://deep.nested.sub.opencode.ai/path?query=1"
        ));
        assert!(is_opencode_host("https://opencode.AI/"));
        assert!(!is_opencode_host("https://notopencode.ai"));
        assert!(!is_opencode_host("https://opencode.ai.evil.com"));
        assert!(!is_opencode_host("https://attacker-opencode.ai"));
        assert!(!is_opencode_host("https://opencode.ai@evil.test"));
        assert!(!is_opencode_host("not-a-url"));
        assert!(!is_opencode_host(""));
    }

    #[test]
    fn the_opencode_gate_covers_the_provider_and_the_host_only() {
        assert!(opencode_headers_apply(
            Some(EndpointProvider::OpencodeGo),
            "http://127.0.0.1:8080"
        ));
        assert!(opencode_headers_apply(
            Some(EndpointProvider::Generic),
            "https://api.opencode.ai"
        ));
        assert!(!opencode_headers_apply(
            Some(EndpointProvider::Generic),
            "http://127.0.0.1:8080"
        ));
        assert!(!opencode_headers_apply(None, "https://example.test"));
    }

    #[test]
    fn the_opencode_session_prefers_the_caller_then_alias_then_conversation_then_request() {
        let request_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        assert_eq!(
            resolve_opencode_session(
                &headers(&[
                    ("x-opencode-session", "caller"),
                    ("x-opencode-session-id", "ses_child"),
                ]),
                Some(conversation_id),
                request_id,
            ),
            "caller"
        );
        assert_eq!(
            resolve_opencode_session(
                &headers(&[("x-opencode-session-id", "ses_child")]),
                Some(conversation_id),
                request_id,
            ),
            "ses_child"
        );
        assert_eq!(
            resolve_opencode_session(&[], Some(conversation_id), request_id),
            format!("ses_{conversation_id}")
        );
        assert_eq!(
            resolve_opencode_session(&[], None, request_id),
            format!("ses_{request_id}")
        );
        assert_eq!(
            resolve_opencode_session(&headers(&[("x-opencode-session", "   ")]), None, request_id),
            format!("ses_{request_id}"),
            "a blank caller session is absent, not a session value"
        );
    }

    #[test]
    fn the_opencode_headers_carry_the_session_the_user_agent_and_safe_identity_headers() {
        let request = apply_opencode_headers(
            reqwest::Client::new().get("https://api.opencode.ai/v1/models"),
            &headers(&[
                ("user-agent", "opencode/1.0 test"),
                ("x-opencode-project", "proj_1"),
                ("authorization", "Bearer secret"),
                ("x-opencode-blank", "   "),
            ]),
            "ses_fixed",
        )
        .build()
        .expect("request");

        assert_eq!(
            request
                .headers()
                .get(OPENCODE_UPSTREAM_SESSION_HEADER)
                .unwrap()
                .to_str()
                .unwrap(),
            "ses_fixed"
        );
        assert_eq!(
            request
                .headers()
                .get(header::USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap(),
            "opencode/1.0 test"
        );
        assert_eq!(
            request
                .headers()
                .get("x-opencode-project")
                .unwrap()
                .to_str()
                .unwrap(),
            "proj_1"
        );
        assert!(request.headers().get("x-opencode-blank").is_none());
        assert!(
            request.headers().get("authorization").is_none(),
            "an authorization header is never forwarded"
        );
    }

    #[test]
    fn the_opencode_headers_synthesize_a_user_agent_without_a_caller_one() {
        let request = apply_opencode_headers(
            reqwest::Client::new().get("https://api.opencode.ai/v1/models"),
            &[],
            "ses_fixed",
        )
        .build()
        .expect("request");

        assert_eq!(
            request
                .headers()
                .get(header::USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
