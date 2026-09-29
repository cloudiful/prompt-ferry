//! Adjacent test module for `upstream_restore.rs` (#569 Phase 2):
//! the JSON restore entry points and their blocking wrappers must be
//! equivalent and keep the strict invalid-token behavior.

use redactor::{FindingKind, InputKind, RedactionPolicy, RedactorBuilder, RestoreState};

use super::{restore_ai_response_json, restore_mcp_body_json};
use crate::redact_upstream::UpstreamRedactionSession;

fn session(original: &str) -> (UpstreamRedactionSession, String) {
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

#[test]
fn restores_valid_tokens_and_preserves_invalid_ai_tokens() {
    let (session, token) = session("a.example.com");
    let text = format!(
        "valid {token} malformed [[RDX:v2:...]] unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
    );
    let body = serde_json::json!({
        "output": [{"type": "output_text", "text": text}]
    });

    let restored = restore_ai_response_json(
        "/v1/responses",
        &serde_json::to_vec(&body).expect("encode"),
        &session,
    )
    .expect("restore");
    let restored: serde_json::Value = serde_json::from_slice(&restored).expect("decode");

    assert_eq!(
        restored["output"][0]["text"],
        "valid a.example.com malformed [[RDX:v2:...]] unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
    );
}

#[test]
fn invalid_ai_json_still_fails() {
    let (session, _) = session("a.example.com");
    assert!(restore_ai_response_json("/v1/responses", br#"{"#, &session).is_err());
}

#[test]
fn mcp_restore_remains_strict_for_invalid_tokens() {
    let (session, _) = session("a.example.com");
    let body = br#"{"text":"[[RDX:v2:...]]"}"#;

    assert!(restore_mcp_body_json(body, &session).is_err());
}

#[test]
fn blocking_ai_restore_matches_the_inline_result() {
    let (session, token) = session("a.example.com");
    let body = serde_json::json!({
        "output": [{"type": "output_text", "text": token}]
    });
    let expected = restore_ai_response_json(
        "/v1/responses",
        &serde_json::to_vec(&body).expect("encode"),
        &session,
    )
    .expect("inline restore");

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let restored = rt
        .block_on(super::restore_ai_response_json_blocking(
            "/v1/responses".to_string(),
            serde_json::to_vec(&body).expect("encode"),
            session,
        ))
        .expect("blocking restore");
    assert_eq!(restored, expected);
    let restored: serde_json::Value = serde_json::from_slice(&restored).expect("decode");
    assert_eq!(restored["output"][0]["text"], "a.example.com");
}

#[test]
fn blocking_ai_restore_propagates_invalid_json() {
    let (session, _) = session("a.example.com");
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let err = rt
        .block_on(super::restore_ai_response_json_blocking(
            "/v1/responses".to_string(),
            b"{".to_vec(),
            session,
        ))
        .expect_err("invalid JSON must fail");
    assert!(!err.to_string().is_empty());
}

#[test]
fn blocking_ai_restore_preserves_the_body_when_the_worker_succeeds() {
    // The blocking wrapper runs on the blocking pool even inside a
    // current-thread runtime: the body must round-trip unchanged.
    let (session, token) = session("a.example.com");
    let body = serde_json::json!({
        "output": [{"type": "output_text", "text": format!("tail {token}")}]
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let restored = rt
        .block_on(super::restore_ai_response_json_blocking(
            "/v1/responses".to_string(),
            serde_json::to_vec(&body).expect("encode"),
            session,
        ))
        .expect("blocking restore");
    let restored: serde_json::Value = serde_json::from_slice(&restored).expect("decode");
    assert_eq!(restored["output"][0]["text"], "tail a.example.com");
}

#[test]
fn blocking_mcp_restore_restores_tokens_and_stays_strict() {
    let (mcp_session, token) = session("a.example.com");
    let body = serde_json::json!({"text": format!("see {token}")});
    let expected = restore_mcp_body_json(&serde_json::to_vec(&body).expect("encode"), &mcp_session)
        .expect("inline restore");

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let restored = rt
        .block_on(super::restore_mcp_body_json_blocking(
            serde_json::to_vec(&body).expect("encode"),
            mcp_session,
        ))
        .expect("blocking restore");
    assert_eq!(restored, expected);
    let restored: serde_json::Value = serde_json::from_slice(&restored).expect("decode");
    assert_eq!(restored["text"], "see a.example.com");

    // Strictness is preserved through the blocking wrapper too.
    let (invalid_session, _) = session("a.example.com");
    let invalid = serde_json::json!({"text": "[[RDX:v2:...]]"});
    assert!(
        rt.block_on(super::restore_mcp_body_json_blocking(
            serde_json::to_vec(&invalid).expect("encode"),
            invalid_session,
        ))
        .is_err()
    );
}
