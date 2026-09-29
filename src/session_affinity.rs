//! Issue #633: shared OpenCode session-affinity alias resolution.
//!
//! Both conversation logging and the Codex OAuth egress resolve the same
//! caller headers to one stable identifier. Precedence is
//! `x-session-id`, `x-session-affinity`, `x-opencode-session`, then
//! `session-id`; blank values are ignored. Distinct non-empty values across
//! aliases count as a conflict — resolution stays deterministic (precedence
//! wins) so logging and upstream never route by different identities.

/// Canonical alias precedence for the stable session identifier.
pub const SESSION_AFFINITY_ALIASES: [&str; 4] = [
    "x-session-id",
    "x-session-affinity",
    "x-opencode-session",
    "session-id",
];

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

/// True when two or more aliases carry distinct non-empty values. Callers
/// keep the precedence winner and must not silently diverge from it.
pub fn has_conflicting_session_affinity(headers: &[(String, String)]) -> bool {
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
    fn unrelated_headers_are_ignored() {
        let request = headers(&[
            ("request-id", "req_1"),
            ("prompt_cache_key", "cache_1"),
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
