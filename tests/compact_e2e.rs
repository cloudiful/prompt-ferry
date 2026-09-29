//! Issue #502 Task 6: `POST /v1/responses/compact` end-to-end dispatch.
//!
//! Covers the three `compact_mode` branches without a live upstream:
//! Responses-native passthrough (bytes preserved, output replays as the next
//! `/v1/responses` input), `self_summarize` local flow for non-Responses
//! targets, and `off` rejecting every target. Usage bore (`normalize` and
//! request text) follows `/v1/responses` for the compact path.

use prompt_ferry::{
    config::NativeApi,
    db::CompactMode,
    upstream_adapter::{
        PreparedRequestBody, ResponseAdapter, prepare_upstream_request,
        prepare_upstream_request_with_compact,
    },
    usage::{extract_request_prompt, normalize_prompt_request},
};

const COMPACT_BODY: &[u8] =
    br#"{"model":"m","input":[{"type":"message","role":"user","content":"hi"}]}"#;

#[test]
fn compact_passthrough_preserves_bytes_and_replays_as_next_input() {
    let prepared = prepare_upstream_request_with_compact(
        "/v1/responses/compact",
        COMPACT_BODY,
        NativeApi::Responses,
        false,
        None,
        CompactMode::Passthrough,
    )
    .unwrap();
    assert_eq!(prepared.path, "/v1/responses/compact");
    assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
    let PreparedRequestBody::PassthroughStream(forwarded) = prepared.body else {
        panic!("compact passthrough must stream the body unchanged");
    };
    assert_eq!(forwarded, COMPACT_BODY);

    // Compact output is replayable as the next `/v1/responses` input.
    let next_input = br#"{"model":"m","input":[{"type":"message","role":"user","content":"hi"},{"type":"message","role":"assistant","content":"handoff summary"}]}"#;
    let replayed = prepare_upstream_request(
        "/v1/responses",
        next_input,
        NativeApi::Responses,
        false,
        None,
    )
    .unwrap();
    assert_eq!(replayed.path, "/v1/responses");
    assert_eq!(replayed.response_adapter, ResponseAdapter::Passthrough);

    // Usage bore matches `/v1/responses`.
    assert!(normalize_prompt_request("/v1/responses/compact", COMPACT_BODY).is_some());
    let prompt = extract_request_prompt("/v1/responses/compact", COMPACT_BODY).unwrap_or_default();
    assert!(prompt.contains("hi"), "compact prompt text: {prompt:?}");
}

#[test]
fn compact_self_summarize_buffers_non_responses_targets_for_local_flow() {
    for native_api in [NativeApi::Chat, NativeApi::AnthropicMessages] {
        let prepared = prepare_upstream_request_with_compact(
            "/v1/responses/compact",
            COMPACT_BODY,
            native_api,
            false,
            None,
            CompactMode::SelfSummarize,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/responses/compact");
        assert_eq!(
            prepared.response_adapter,
            ResponseAdapter::SelfSummarizeLocal,
            "native_api={native_api:?}"
        );
        let PreparedRequestBody::BufferedBytes(buffered) = prepared.body else {
            panic!("self-summarize compact must buffer for the local flow");
        };
        assert_eq!(buffered, COMPACT_BODY);
    }

    // Responses-native targets never take the local flow.
    let prepared = prepare_upstream_request_with_compact(
        "/v1/responses/compact",
        COMPACT_BODY,
        NativeApi::Responses,
        false,
        None,
        CompactMode::SelfSummarize,
    )
    .unwrap();
    assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
}

#[test]
fn compact_off_rejects_every_target() {
    for native_api in [
        NativeApi::Responses,
        NativeApi::Chat,
        NativeApi::AnthropicMessages,
        NativeApi::Auto,
    ] {
        let error = prepare_upstream_request_with_compact(
            "/v1/responses/compact",
            COMPACT_BODY,
            native_api,
            false,
            None,
            CompactMode::Off,
        )
        .unwrap_err();
        assert_eq!(error.code, "compact_disabled", "native_api={native_api:?}");
    }
}

#[test]
fn routeless_responses_to_chat_rejects_caller_tier() {
    // Issue #637: the route-less entry points carry no provider context, so
    // a caller `service_tier` is denied by default exactly like an
    // unsupported provider — rejected here on Responses→Chat.
    let error = prepare_upstream_request(
        "/v1/responses",
        br#"{"model":"m","input":"hi","service_tier":"priority"}"#,
        NativeApi::Chat,
        false,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "unsupported_feature");
    assert!(error.message.contains("service_tier"));
}

#[test]
fn routeless_chat_to_responses_forwards_caller_tier() {
    // Issue #637: caller-compatibility baseline — the route-less entry
    // point forwards the caller field on Chat→Responses for every provider
    // (no configured value preserves the caller's field); the
    // configuration-based override stays gated in the live transform.
    let prepared = prepare_upstream_request_with_compact(
        "/v1/chat/completions",
        br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"service_tier":"priority"}"#,
        NativeApi::Responses,
        false,
        None,
        CompactMode::Passthrough,
    )
    .unwrap();
    assert_eq!(prepared.path, "/v1/responses");
    let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
        panic!("chat to Responses translation must buffer");
    };
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["service_tier"].as_str(), Some("priority"));
}

#[test]
fn compact_passthrough_rejects_cross_protocol_targets_by_default() {
    for native_api in [
        NativeApi::Chat,
        NativeApi::AnthropicMessages,
        NativeApi::Auto,
    ] {
        let error = prepare_upstream_request_with_compact(
            "/v1/responses/compact",
            COMPACT_BODY,
            native_api,
            false,
            None,
            CompactMode::Passthrough,
        )
        .unwrap_err();
        assert_eq!(
            error.code, "responses_cross_protocol_unsupported",
            "native_api={native_api:?}"
        );
    }
}
