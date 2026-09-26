//! Issue #599 R2f.1: Codex subscription request-context header parity.
//!
//! These builder tests live next to `upstream.rs` instead of inside its
//! oversized inline test module: caller `session-id` mirroring, access-token
//! compute residency, and the untouched platform API-key path are covered
//! here; credential/body contracts live in `codex_auth_tests`.

use super::codex_test_support::{codex_request, jwt_with_claims, openai_responses_route};
use super::*;

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
