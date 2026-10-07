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

/// OpenCode 2.0.20+ header naming the session that owns the request.
pub const OPENCODE_SESSION_HEADER: &str = "x-opencode-session-id";

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
}
