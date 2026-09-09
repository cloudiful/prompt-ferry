//! Tests for `glm_envelope` (issue #241).
//!
//! Split from the main module to keep its production surface under
//! the 400-line hard cap. The tests live as a sibling so they can
//! reach the `pub(super)` `glm_envelope_code_and_message` helper
//! via `super::glm_envelope::*`.

use super::glm_envelope::{
    check_glm_envelope_error, envelope_preflight_eligible, glm_envelope_code_and_message,
};
use crate::{config::NativeApi, db::EndpointProvider, upstream_adapter::ResponseAdapter};

#[test]
fn detects_live_zhipu_404_envelope() {
    // The exact live failure mode the runtime used to swallow as
    // an empty success: a 2xx HTTP status carrying
    // `{code: 500, msg: "404 NOT_FOUND", success: false}`.
    let body = br#"{"code":500,"msg":"404 NOT_FOUND","success":false,"data":null}"#;
    let err = check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Responses)
        .expect("envelope must be flagged");
    assert_eq!(err.code, "glm_envelope_error");
    assert_eq!(err.status, http::StatusCode::BAD_GATEWAY);
    assert!(
        err.message.contains("404 NOT_FOUND"),
        "message must surface the upstream reason, got: {}",
        err.message,
    );
    assert!(
        err.message.contains("500"),
        "message must surface the envelope code, got: {}",
        err.message,
    );
}

#[test]
fn detects_envelope_on_glm_chat_path() {
    // The same dead-route envelope on the GLM Chat arm — the
    // Chat native API is also covered because the runtime
    // translates Chat responses to Responses for the client.
    let body = br#"{"code":500,"msg":"404 NOT_FOUND","success":false}"#;
    let err = check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Chat)
        .expect("envelope must be flagged on Chat arm too");
    assert_eq!(err.code, "glm_envelope_error");
}

#[test]
fn passes_through_normal_responses_object() {
    // A standard Responses object must NOT trip the check; the
    // runtime's existing translation pipeline must continue to
    // see the body unchanged.
    let body =
        br#"{"id":"resp_1","object":"response","status":"completed","output":[],"usage":{"total_tokens":5}}"#;
    assert!(
        check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Responses).is_none(),
        "normal Responses body must not be flagged",
    );
}

#[test]
fn passes_through_normal_chat_object() {
    let body =
        br#"{"id":"chatcmpl-1","object":"chat.completion","choices":[],"usage":{"total_tokens":5}}"#;
    assert!(
        check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Chat).is_none(),
        "normal Chat body must not be flagged",
    );
}

#[test]
fn zero_or_two_hundred_code_passes_through() {
    // The 0/200 success convention: an envelope with
    // `code: 0` is the success shape and must not be flagged,
    // even though it carries `success: true`. The same applies
    // to the rare `code: 200` shape. The msg field is allowed
    // to carry non-ASCII text (Zhipu's "操作成功" success
    // sentinel) without tripping the check.
    let bodies: Vec<&[u8]> = vec![
        br#"{"code":0,"msg":"success","success":true,"data":{}}"#,
        br#"{"code":200,"msg":"ok","success":true}"#,
    ];
    for body in bodies {
        assert!(
            check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Responses).is_none(),
            "code 0/200 envelope must pass through: {}",
            String::from_utf8_lossy(body),
        );
    }
}

#[test]
fn success_false_without_code_fails_loudly_when_message_missing() {
    // Issue #241 P3-1 (rename only — logic was correct): an envelope
    // with `success: false` but no `msg`/`message` still surfaces a
    // generic operator-facing message so the call is not silently
    // recorded as success. The previous name
    // `..._passes_through_...` inverted the assertion intent and
    // hid the loud-fail contract from future readers.
    let body = br#"{"success":false}"#;
    let err = check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Responses)
        .expect("success=false without code must be flagged");
    assert_eq!(err.code, "glm_envelope_error");
    assert!(err.message.contains("GLM rejected"));
}

#[test]
fn non_glm_responses_body_passes_through() {
    // The check is GLM-scoped: standard OpenAI / MiniMax /
    // CommandCode / OpencodeGo / OpenRouter providers must
    // continue to forward 2xx bodies without an envelope check.
    let body = br#"{"code":500,"msg":"404 NOT_FOUND","success":false}"#;
    for provider in [
        EndpointProvider::Generic,
        EndpointProvider::Minimax,
        EndpointProvider::CommandCode,
        EndpointProvider::OpencodeGo,
        EndpointProvider::OpenRouter,
    ] {
        assert!(
            check_glm_envelope_error(body, provider, NativeApi::Responses).is_none(),
            "{provider:?} must not see the GLM envelope check",
        );
    }
}

#[test]
fn glm_anthropic_arm_is_not_envelope_checked() {
    // GLM's Anthropic arm is a native Anthropic bridge; the
    // upstream body is Anthropic-shape, never a Zhipu envelope.
    // The check must not fire even on an envelope-shaped body
    // (e.g. a misconfigured upstream returning a Zhipu
    // envelope-shaped error).
    let body = br#"{"code":500,"msg":"404 NOT_FOUND","success":false}"#;
    assert!(
        check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::AnthropicMessages)
            .is_none(),
        "Anthropic arm must not be envelope-checked",
    );
}

#[test]
fn non_json_body_passes_through() {
    // Non-JSON bodies cannot be envelope errors; the check must
    // not flag them so the existing translation pipeline sees
    // the body unchanged.
    let body = b"not-a-json-body";
    assert!(check_glm_envelope_error(body, EndpointProvider::Glm, NativeApi::Responses).is_none(),);
}

#[test]
fn envelope_code_and_message_helper_uses_msg_field() {
    let value = serde_json::json!({"code": 500, "msg": "404 NOT_FOUND", "success": false});
    let (code, message) = glm_envelope_code_and_message(&value).expect("envelope");
    assert_eq!(code.as_deref(), Some("500"));
    assert_eq!(message, "404 NOT_FOUND");
}

#[test]
fn envelope_code_and_message_helper_falls_back_to_message() {
    let value = serde_json::json!({"code": 500, "message": "alt msg"});
    let (code, message) = glm_envelope_code_and_message(&value).expect("envelope");
    assert_eq!(code.as_deref(), Some("500"));
    assert_eq!(message, "alt msg");
}

#[test]
fn envelope_code_and_message_helper_returns_none_on_success() {
    for value in [
        serde_json::json!({"code": 0, "msg": "ok", "success": true}),
        serde_json::json!({"code": 200, "msg": "ok", "success": true}),
        serde_json::json!({"id": "resp_1", "object": "response"}),
    ] {
        assert!(
            glm_envelope_code_and_message(&value).is_none(),
            "value {value} must not be flagged",
        );
    }
}

#[test]
fn envelope_preflight_eligible_matches_only_passthrough_non_sse_glm_json() {
    // The centralized preflight covers the Passthrough + non-SSE
    // + GLM Chat/Responses + JSON path. Translation branches
    // (ChatToResponses / ResponsesToChat) and the Anthropic arm
    // perform their own per-forwarder check, so they must NOT
    // trigger the centralized read (which would otherwise read
    // the body twice). SSE and non-JSON responses are streamed
    // and cannot be peeked here.
    let eligible = (
        EndpointProvider::Glm,
        NativeApi::Chat,
        Some("application/json"),
        false,
        ResponseAdapter::Passthrough,
    );
    assert!(envelope_preflight_eligible(
        eligible.0, eligible.1, eligible.2, eligible.3, eligible.4
    ));
    let eligible_responses = (
        EndpointProvider::Glm,
        NativeApi::Responses,
        Some("application/json; charset=utf-8"),
        false,
        ResponseAdapter::Passthrough,
    );
    assert!(envelope_preflight_eligible(
        eligible_responses.0,
        eligible_responses.1,
        eligible_responses.2,
        eligible_responses.3,
        eligible_responses.4
    ));
    for (provider, native_api, content_type, is_sse, adapter) in [
        // Non-GLM providers must never preflight.
        (
            EndpointProvider::Minimax,
            NativeApi::Chat,
            Some("application/json"),
            false,
            ResponseAdapter::Passthrough,
        ),
        (
            EndpointProvider::Generic,
            NativeApi::Responses,
            Some("application/json"),
            false,
            ResponseAdapter::Passthrough,
        ),
        // SSE responses cannot be peeked for envelope.
        (
            EndpointProvider::Glm,
            NativeApi::Chat,
            Some("text/event-stream"),
            true,
            ResponseAdapter::Passthrough,
        ),
        // Translation branches do their own check; centralization must not double-read.
        (
            EndpointProvider::Glm,
            NativeApi::Chat,
            Some("application/json"),
            false,
            ResponseAdapter::ChatToResponses,
        ),
        (
            EndpointProvider::Glm,
            NativeApi::Responses,
            Some("application/json"),
            false,
            ResponseAdapter::ResponsesToChat,
        ),
        (
            EndpointProvider::Glm,
            NativeApi::Responses,
            Some("application/json"),
            false,
            ResponseAdapter::AnthropicMessagesToResponses,
        ),
        // Anthropic native API does not carry the Zhipu envelope.
        (
            EndpointProvider::Glm,
            NativeApi::AnthropicMessages,
            Some("application/json"),
            false,
            ResponseAdapter::Passthrough,
        ),
        // Non-JSON content types are not envelope-shaped.
        (
            EndpointProvider::Glm,
            NativeApi::Chat,
            Some("text/plain"),
            false,
            ResponseAdapter::Passthrough,
        ),
        (
            EndpointProvider::Glm,
            NativeApi::Chat,
            None,
            false,
            ResponseAdapter::Passthrough,
        ),
    ] {
        assert!(
            !envelope_preflight_eligible(provider, native_api, content_type, is_sse, adapter),
            "preflight must not apply for {provider:?}/{native_api:?} \
             content_type={content_type:?} is_sse={is_sse} adapter={adapter:?}",
        );
    }
}
