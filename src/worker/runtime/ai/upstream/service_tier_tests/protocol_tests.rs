//! Issue #637/#644: service-tier protocol and forwarding parity.
//!
//! Every provider receives the configured override as a best-effort top-level
//! `service_tier` passthrough on the HTTP JSON protocols (Chat Completions,
//! Responses, Anthropic Messages); Realtime is excluded. The platform builder
//! and the Codex backend carry it the same way as the direct transform.

use std::borrow::Cow;

use super::*;

#[test]
fn chat_and_responses_bodies_receive_configured_tier() {
    // The top-level `service_tier` field is injected verbatim for every
    // provider on both protocols.
    for provider in ALL_PROVIDERS {
        for native_api in [NativeApi::Chat, NativeApi::Responses] {
            let route =
                route_with_protocol(&responses_route(provider, Some("priority")), native_api);
            let injected = apply_service_tier_override(&route, br#"{"model":"m"}"#);
            let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
            assert_eq!(
                value["service_tier"], "priority",
                "{provider:?} {native_api:?}"
            );
            let overwrite =
                apply_service_tier_override(&route, br#"{"model":"m","service_tier":"standard"}"#);
            let value: serde_json::Value = serde_json::from_slice(overwrite.as_ref()).unwrap();
            assert_eq!(
                value["service_tier"], "priority",
                "{provider:?} {native_api:?}"
            );
        }
    }
}

#[test]
fn anthropic_messages_receives_configured_tier_for_every_provider() {
    // Issue #644: Anthropic Messages is an HTTP JSON protocol too, so the
    // provider-agnostic override applies there as well.
    for provider in ALL_PROVIDERS {
        let route = route_with_protocol(
            &responses_route(provider, Some("priority")),
            NativeApi::AnthropicMessages,
        );
        let injected = apply_service_tier_override(&route, br#"{"model":"m"}"#);
        let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
        assert_eq!(value["service_tier"], "priority", "{provider:?}");
    }
}

#[test]
fn realtime_bodies_leave_body_unchanged_for_every_provider() {
    // Realtime carries WebSocket frames instead of the common JSON request
    // body — bodies pass through byte-for-byte even when a tier is configured.
    for provider in ALL_PROVIDERS {
        let route = route_with_protocol(
            &responses_route(provider, Some("priority")),
            NativeApi::Realtime,
        );
        for body in [
            br#"{"model":"m"}"#.as_slice(),
            br#"{"model":"m","service_tier":"standard"}"#.as_slice(),
        ] {
            let injected = apply_service_tier_override(&route, body);
            assert!(matches!(&injected, Cow::Borrowed(_)));
            assert_eq!(injected.as_ref(), body, "{provider:?}");
        }
    }
}

#[test]
fn untiered_routes_preserve_caller_across_protocols() {
    // With no configured override the caller field survives on every
    // protocol; the transform never strips or rewrites it.
    for provider in ALL_PROVIDERS {
        for native_api in [
            NativeApi::Chat,
            NativeApi::Responses,
            NativeApi::AnthropicMessages,
        ] {
            let route = route_with_protocol(&responses_route(provider, None), native_api);
            let body = br#"{"model":"m","service_tier":"standard"}"#;
            let injected = apply_service_tier_override(&route, body);
            assert!(matches!(&injected, Cow::Borrowed(_)));
            assert_eq!(injected.as_ref(), body, "{provider:?} {native_api:?}");
        }
    }
}

#[test]
fn build_upstream_request_applies_tier_for_chat_and_responses() {
    for (native_api, url) in [
        (
            NativeApi::Chat,
            "https://api.openai.com/v1/chat/completions",
        ),
        (NativeApi::Responses, "https://api.openai.com/v1/responses"),
    ] {
        let route = route_with_protocol(
            &responses_route(EndpointProvider::OpenAi, Some("fast")),
            native_api,
        );
        for body in [
            PreparedRequestBody::BufferedBytes(br#"{"model":"gpt-5"}"#.to_vec()),
            PreparedRequestBody::PassthroughStream(br#"{"model":"gpt-5"}"#.to_vec()),
        ] {
            let request = build_upstream_request(
                &Client::new(),
                &Method::POST,
                url,
                &route,
                match native_api {
                    NativeApi::Chat => "/v1/chat/completions",
                    _ => "/v1/responses",
                },
                &body,
                &[],
                None,
            )
            .build()
            .unwrap();
            let bytes = request
                .body()
                .and_then(|body| body.as_bytes())
                .expect("upstream body bytes");
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(value["service_tier"], "fast", "{native_api:?}");
        }
    }
    // An untiered route preserves the caller field verbatim.
    let untiered = responses_route(EndpointProvider::OpenAi, None);
    let request = build_upstream_request(
        &Client::new(),
        &Method::POST,
        "https://api.openai.com/v1/responses",
        &untiered,
        "/v1/responses",
        &PreparedRequestBody::BufferedBytes(br#"{"model":"gpt-5","service_tier":"auto"}"#.to_vec()),
        &[],
        None,
    )
    .build()
    .unwrap();
    let bytes = request
        .body()
        .and_then(|body| body.as_bytes())
        .expect("untiered body bytes");
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(value["service_tier"], "auto");
}

#[test]
fn codex_backend_preserves_configured_tier_through_normalization() {
    // The Codex backend normalizes `model` and forces `store: false`; a
    // configured tier must survive that rewrite on the same Responses body.
    let route = responses_route(EndpointProvider::OpenAi, Some("fast"));
    let request = build_codex_upstream_request(
        &Client::new(),
        &Method::POST,
        "https://chatgpt.com/backend-api/codex/responses",
        &route,
        "/v1/responses",
        &PreparedRequestBody::BufferedBytes(br#"{"model":"gpt-4o"}"#.to_vec()),
        &CodexAuth::new("oauth-access-token".to_string(), false),
        &[],
    )
    .build()
    .unwrap();
    let bytes = request
        .body()
        .and_then(|body| body.as_bytes())
        .expect("codex body bytes");
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(value["service_tier"], "fast");
    assert_eq!(value["store"], false);
    assert_eq!(value["model"], "gpt-4o");
}
