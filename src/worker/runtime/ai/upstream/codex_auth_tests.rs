//! Issue #599 R2f.1: Codex credential and body contracts.
//!
//! These builder tests moved out of `upstream.rs` and now live next to
//! `codex_header_tests.rs`: OAuth bearer auth, the account-claim binding, the
//! byte-stable body of a credential-only request, and credential redaction.

use super::codex_test_support::{codex_request, jwt_with_claims};
use super::*;

#[test]
fn codex_request_uses_oauth_bearer_and_normalizes_the_model() {
    let request = codex_request(
        br#"{"model":"gpt-4o","input":[{"role":"user","content":"hi"}]}"#,
        &[],
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(
        request.headers().get(header::AUTHORIZATION).unwrap(),
        "Bearer oauth-access-token"
    );
    assert_eq!(request.headers().get("originator").unwrap(), "opencode");
    // An opaque token carries no account claim, so no binding header.
    assert!(request.headers().get("chatgpt-account-id").is_none());
    let value: serde_json::Value =
        serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    // Unknown models pass through so the backend returns the true error.
    assert_eq!(value["model"], "gpt-4o");
    assert_eq!(value["store"], false);
    assert_eq!(value["input"][0]["content"], "hi");
}

#[test]
fn codex_request_carries_the_account_id_claim() {
    let token = jwt_with_claims(serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_account_id": "acct_123" }
    }));
    let auth = CodexAuth::new(token, true);
    assert_eq!(auth.account_id.as_deref(), Some("acct_123"));
    assert!(auth.refreshed);
    let request = codex_request(br#"{"model":"gpt-5.1-codex"}"#, &[], &auth);
    assert_eq!(
        request.headers().get("chatgpt-account-id").unwrap(),
        "acct_123"
    );
}

#[test]
fn codex_body_normalization_borrows_an_already_normalized_body() {
    let body = br#"{ "model" : "gpt-5.1-codex" , "store" : false }"#;
    let request = codex_request(
        body,
        &[],
        &CodexAuth::new("oauth-access-token".to_string(), false),
    );
    assert_eq!(
        request.body().unwrap().as_bytes().unwrap(),
        body,
        "an already-normalized body must stay byte-for-byte"
    );
}

#[test]
fn codex_auth_debug_redacts_the_access_token() {
    let token = jwt_with_claims(serde_json::json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct_123",
            "chatgpt_compute_residency": "future-region_1",
        }
    }));
    let auth = CodexAuth::new(token.clone(), false);
    let debug = format!("{auth:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(&token));
    // The residency claim is derived per request and never stored on the
    // credential, so it cannot leak through debug output either.
    assert!(!debug.contains("future-region_1"));
}
