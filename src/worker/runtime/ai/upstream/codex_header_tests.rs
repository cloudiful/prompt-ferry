//! Issue #599 R2f.1 + #633 + #701: Codex subscription request-context header
//! parity.
//!
//! These builder tests live next to `upstream.rs` instead of inside its
//! oversized inline test module: caller session-affinity alias resolution
//! (including the OpenCode V2 child/parent header combination), access-token
//! compute residency, and the untouched platform API-key path are covered
//! here; credential/body contracts live in `codex_auth_tests`.

use super::codex_test_support::{codex_request, jwt_with_claims, openai_responses_route};
use super::*;
use crate::session_affinity::{has_conflicting_session_affinity, resolve_session_affinity};

#[test]
fn codex_request_mirrors_the_caller_session_id_only() {
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &[
            ("Session-Id".to_string(), "  session-1  ".to_string()),
            ("cookie".to_string(), "sid=attacker".to_string()),
            ("authorization".to_string(), "Bearer attacker".to_string()),
        ],
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(
        request.headers().get("session-id").unwrap(),
        "session-1",
        "the stable caller session id is mirrored, trimmed"
    );
    assert_eq!(
        request.headers().get(header::AUTHORIZATION).unwrap(),
        "Bearer oauth-access-token",
        "unrelated caller headers never override the subscription credential"
    );
    assert!(request.headers().get(header::COOKIE).is_none());
}

#[test]
fn codex_request_omits_absent_blank_or_per_turn_session_ids() {
    let cases: [Vec<(String, String)>; 3] = [
        Vec::new(),
        vec![("session-id".to_string(), "   ".to_string())],
        vec![("request-id".to_string(), "req_per_turn".to_string())],
    ];
    for headers in cases {
        let request = codex_request(
            br#"{"model":"gpt-5.1-codex"}"#,
            &headers,
            &CodexAuth::new("oauth-access-token".to_string(), false),
        );
        assert!(
            request.headers().get("session-id").is_none(),
            "no session id may be synthesized; headers: {headers:?}"
        );
    }
}

#[test]
fn codex_request_forwards_the_compute_residency_claim() {
    let nested = jwt_with_claims(serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_compute_residency": "eu" }
    }));
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &[],
        &CodexAuth::new(nested, false),
    );
    assert_eq!(
        request
            .headers()
            .get("x-openai-internal-codex-residency")
            .unwrap(),
        "eu"
    );

    let top_level = jwt_with_claims(serde_json::json!({
        "chatgpt_compute_residency": "future-region_1"
    }));
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &[],
        &CodexAuth::new(top_level, false),
    );
    assert_eq!(
        request
            .headers()
            .get("x-openai-internal-codex-residency")
            .unwrap(),
        "future-region_1",
        "a top-level claim is a supported fallback"
    );
}

#[test]
fn codex_request_filters_no_constraint_and_unreadable_residency() {
    let unconstrained = jwt_with_claims(serde_json::json!({
        "chatgpt_compute_residency": "eu",
        "https://api.openai.com/auth": { "chatgpt_compute_residency": "no_constraint" }
    }));
    let flat_unconstrained = jwt_with_claims(serde_json::json!({
        "chatgpt_compute_residency": "no_constraint"
    }));
    let blank = jwt_with_claims(serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_compute_residency": "  " }
    }));
    for token in [
        unconstrained,
        flat_unconstrained,
        blank,
        "opaque-token".to_string(),
        "header.!!!.signature".to_string(),
    ] {
        let request = codex_request(
            br#"{"model":"gpt-5.1-codex"}"#,
            &[],
            &CodexAuth::new(token.clone(), false),
        );
        assert!(
            request
                .headers()
                .get("x-openai-internal-codex-residency")
                .is_none(),
            "residency must be skipped for token {token}"
        );
    }
}

#[test]
fn platform_builder_keeps_the_platform_model_and_store_field() {
    // The Codex model mapping is only reachable through the Codex builder; a
    // platform OpenAI Responses route must forward its body unchanged and must
    // never gain Codex subscription headers.
    let route = openai_responses_route();
    let request = build_upstream_request(
        &Client::new(),
        &Method::POST,
        "https://api.openai.com/v1/responses",
        &route,
        &PreparedRequestBody::BufferedBytes(br#"{"model":"gpt-4o","store":true}"#.to_vec()),
        &[("session-id".to_string(), "session-1".to_string())],
        None,
    )
    .build()
    .unwrap();
    assert_eq!(
        request.headers().get(header::AUTHORIZATION).unwrap(),
        "Bearer platform-key"
    );
    assert!(request.headers().get("session-id").is_none());
    assert!(
        request
            .headers()
            .get("x-openai-internal-codex-residency")
            .is_none()
    );
    let value: serde_json::Value =
        serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(value["model"], "gpt-4o");
    assert_eq!(value["store"], true);
}

// Issue #633: shared alias resolution for the Codex OAuth egress.

#[test]
fn codex_request_resolves_each_session_alias() {
    for (header_name, header_value) in [
        ("x-opencode-session-id", "ses_child"),
        ("x-session-id", "ses_explicit"),
        ("x-session-affinity", "ses_affinity"),
        ("x-opencode-session", "ses_opencode"),
        ("session-id", "ses_legacy"),
    ] {
        let headers = vec![(header_name.to_string(), header_value.to_string())];
        let request = codex_request(
            br#"{"model":"gpt-5.1-codex"}"#,
            &headers,
            &CodexAuth::new("oauth-access-token".to_string(), false),
        );
        assert_eq!(
            request.headers().get("session-id").unwrap(),
            header_value,
            "alias {header_name} must forward as upstream session-id"
        );
        assert_eq!(
            resolve_session_affinity(&headers).as_deref(),
            Some(header_value),
            "upstream must match the shared resolver for {header_name}"
        );
    }
}

#[test]
fn codex_request_forwards_the_opencode_v2_child_session_id() {
    // Issue #701: the upstream Codex session id must follow the child session
    // OpenCode V2 reports, not the parent its lineage-root aliases carry.
    let headers = vec![
        ("x-opencode-session-id".to_string(), "ses_child".to_string()),
        (
            "x-opencode-parent-session-id".to_string(),
            "ses_parent".to_string(),
        ),
        ("x-session-affinity".to_string(), "ses_parent".to_string()),
        ("X-Session-Id".to_string(), "ses_parent".to_string()),
        ("x-opencode-session".to_string(), "ses_parent".to_string()),
        ("x-parent-session-id".to_string(), "ses_parent".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_child");
    assert_eq!(
        resolve_session_affinity(&headers).as_deref(),
        Some("ses_child")
    );
    assert!(
        !has_conflicting_session_affinity(&headers),
        "the expected parent-valued lineage aliases must not look like a conflict"
    );
}

#[test]
fn codex_request_prefers_alias_precedence() {
    let headers = vec![
        ("session-id".to_string(), "ses_legacy".to_string()),
        ("x-opencode-session".to_string(), "ses_opencode".to_string()),
        ("x-session-affinity".to_string(), "ses_affinity".to_string()),
        ("x-session-id".to_string(), "ses_explicit".to_string()),
        ("x-opencode-session-id".to_string(), "ses_child".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_child");
    assert_eq!(
        resolve_session_affinity(&headers).as_deref(),
        Some("ses_child")
    );

    let legacy = vec![
        ("session-id".to_string(), "ses_legacy".to_string()),
        ("x-opencode-session".to_string(), "ses_opencode".to_string()),
        ("x-session-affinity".to_string(), "ses_affinity".to_string()),
        ("x-session-id".to_string(), "ses_explicit".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &legacy,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_explicit");

    let fallback = vec![
        ("x-session-affinity".to_string(), "ses_affinity".to_string()),
        ("x-opencode-session".to_string(), "ses_opencode".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &fallback,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_affinity");
}

#[test]
fn codex_request_ignores_blank_aliases() {
    let headers = vec![
        ("x-opencode-session-id".to_string(), "   ".to_string()),
        ("x-session-id".to_string(), "   ".to_string()),
        ("x-session-affinity".to_string(), String::new()),
        ("x-opencode-session".to_string(), "ses_ok".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_ok");
}

#[test]
fn codex_request_conflict_forwards_the_precedence_winner() {
    let headers = vec![
        ("x-session-id".to_string(), "ses_a".to_string()),
        ("x-session-affinity".to_string(), "ses_b".to_string()),
        ("x-opencode-session".to_string(), "ses_c".to_string()),
    ];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex"}"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    let forwarded = request.headers().get("session-id").unwrap();
    assert_eq!(forwarded, "ses_a");
    assert_eq!(
        resolve_session_affinity(&headers).as_deref(),
        Some(forwarded.to_str().unwrap()),
        "conflicting aliases must not silently diverge from the shared resolver"
    );
}

#[test]
fn codex_request_never_derives_session_from_body_or_per_turn_ids() {
    let headers = vec![
        ("request-id".to_string(), "req_per_turn".to_string()),
        ("x-request-id".to_string(), "req_other".to_string()),
    ];
    let request = codex_request(
        br#"{
            "model": "gpt-5.1-codex",
            "prompt_cache_key": "cache_123",
            "client_metadata": {"x-codex-window-id": "thread_1:0"}
        }"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert!(request.headers().get("session-id").is_none());
}

#[test]
fn codex_request_preserves_prompt_cache_key() {
    let headers = vec![("x-session-affinity".to_string(), "ses_123".to_string())];
    let request = codex_request(
        br#"{"model":"gpt-5.1-codex","store":true,"prompt_cache_key":"cache_123"}"#,
        &headers,
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(request.headers().get("session-id").unwrap(), "ses_123");
    let value: serde_json::Value =
        serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(value["prompt_cache_key"], "cache_123");
}

#[test]
fn platform_builder_ignores_all_session_aliases() {
    let route = openai_responses_route();
    let request = build_upstream_request(
        &Client::new(),
        &Method::POST,
        "https://api.openai.com/v1/responses",
        &route,
        &PreparedRequestBody::BufferedBytes(br#"{"model":"gpt-4o","store":true}"#.to_vec()),
        &[
            ("x-opencode-session-id".to_string(), "ses_child".to_string()),
            ("x-session-id".to_string(), "ses_explicit".to_string()),
            ("x-session-affinity".to_string(), "ses_affinity".to_string()),
            ("x-opencode-session".to_string(), "ses_opencode".to_string()),
            ("session-id".to_string(), "ses_legacy".to_string()),
        ],
        None,
    )
    .build()
    .unwrap();
    assert!(request.headers().get("session-id").is_none());
    assert!(
        request
            .headers()
            .get("x-openai-internal-codex-residency")
            .is_none()
    );
}
