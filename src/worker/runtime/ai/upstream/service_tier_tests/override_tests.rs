//! Issue #637/#644: core service-tier override semantics.
//!
//! Configured tiers inject and overwrite on every provider; unset/blank tiers
//! preserve the caller body byte-for-byte; Realtime and non-JSON bodies pass
//! through untouched. Protocol coverage lives in [`super::protocol_tests`].

use std::borrow::Cow;

use super::*;

#[test]
fn minimax_injects_configured_tier_over_body_value() {
    let route = responses_route(EndpointProvider::Minimax, Some("priority"));
    let body = br#"{"model":"MiniMax-M2","service_tier":"standard"}"#;
    let injected = apply_service_tier_override(&route, body);
    let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
    assert_eq!(value["service_tier"], "priority");
    assert_eq!(value["model"], "MiniMax-M2");
    assert!(matches!(&injected, Cow::Owned(_)));
}

#[test]
fn minimax_injects_tier_when_body_omits_field() {
    let route = responses_route(EndpointProvider::Minimax, Some("standard"));
    let injected = apply_service_tier_override(&route, br#"{"model":"MiniMax-M2"}"#);
    let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
    assert_eq!(value["service_tier"], "standard");
}

#[test]
fn openai_injects_configured_tier_over_body_value() {
    // OpenAI shares the wire key but not the value vocabulary: the free-form
    // `fast` override replaces the caller `auto` verbatim.
    let route = responses_route(EndpointProvider::OpenAi, Some("fast"));
    let body = br#"{"model":"gpt-5","service_tier":"auto"}"#;
    let injected = apply_service_tier_override(&route, body);
    let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
    assert_eq!(value["service_tier"], "fast");
    assert_eq!(value["model"], "gpt-5");
    assert!(matches!(&injected, Cow::Owned(_)));
}

#[test]
fn openai_preserves_free_form_values_verbatim() {
    let route = responses_route(EndpointProvider::OpenAi, Some("priority"));
    let injected = apply_service_tier_override(&route, br#"{"model":"gpt-5"}"#);
    let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
    assert_eq!(value["service_tier"], "priority");
}

#[test]
fn unset_tier_preserves_caller_field_for_every_provider() {
    // Issue #637/#644: with no configured override the caller's field/provider
    // default is preserved; the body is forwarded byte-for-byte.
    for provider in ALL_PROVIDERS {
        let route = responses_route(provider, None);
        for body in [
            br#"{"model":"m"}"#.as_slice(),
            br#"{"model":"m","service_tier":"priority"}"#.as_slice(),
        ] {
            let injected = apply_service_tier_override(&route, body);
            assert!(matches!(&injected, Cow::Borrowed(_)));
            assert_eq!(injected.as_ref(), body, "{provider:?}");
        }
    }
}

#[test]
fn blank_tier_is_treated_as_unset() {
    // The persistence boundary already normalizes blanks to `None`; a blank
    // value reaching the transform must never inject an empty tier.
    for tier in [Some(""), Some("   ")] {
        let route = responses_route(EndpointProvider::Minimax, tier);
        let body = br#"{"model":"m","service_tier":"priority"}"#;
        let injected = apply_service_tier_override(&route, body);
        assert!(matches!(&injected, Cow::Borrowed(_)));
        assert_eq!(injected.as_ref(), body);
    }
}

#[test]
fn every_provider_injects_on_json_protocols() {
    // Issue #644: the configured override is provider-agnostic — it is a
    // best-effort passthrough for any provider rather than a MiniMax/OpenAI
    // allowlist.
    for provider in ALL_PROVIDERS {
        let route = responses_route(provider, Some("priority"));
        let body = br#"{"model":"m","service_tier":"standard"}"#;
        let injected = apply_service_tier_override(&route, body);
        let value: serde_json::Value = serde_json::from_slice(injected.as_ref()).unwrap();
        assert_eq!(value["service_tier"], "priority", "{provider:?}");
        assert_eq!(value["model"], "m", "{provider:?}");
    }
}

#[test]
fn non_json_bodies_pass_through_unchanged() {
    let route = responses_route(EndpointProvider::Minimax, Some("priority"));
    let body = b"not-json";
    assert_eq!(apply_service_tier_override(&route, body).as_ref(), body);
    let array_body = b"[1,2,3]";
    assert_eq!(
        apply_service_tier_override(&route, array_body).as_ref(),
        array_body
    );
}

#[test]
fn already_configured_tier_skips_reserialization() {
    // Deliberately non-canonical key order/whitespace: an already-correct
    // tier must be forwarded byte-for-byte, not re-serialized (issue #259
    // prefix-cache stability).
    for provider in ALL_PROVIDERS {
        let route = responses_route(provider, Some("priority"));
        let body = br#"{ "service_tier" : "priority" , "model" : "m" }"#;
        let injected = apply_service_tier_override(&route, body);
        assert!(matches!(&injected, Cow::Borrowed(_)));
        assert_eq!(injected.as_ref(), body, "{provider:?}");
    }
}

#[test]
fn reserialization_is_deterministic_and_key_sorted() {
    let route = responses_route(EndpointProvider::Minimax, Some("priority"));
    // serde_json's default Map is a BTreeMap (no `preserve_order` feature
    // in Cargo.toml), so keys are emitted in sorted order deterministically.
    let body = br#"{"z":1,"model":"m","a":{"b":1,"a":2}}"#;
    let first = apply_service_tier_override(&route, body);
    let second = apply_service_tier_override(&route, body);
    assert_eq!(first.as_ref(), second.as_ref());
    assert_eq!(
        first.as_ref(),
        br#"{"a":{"a":2,"b":1},"model":"m","service_tier":"priority","z":1}"#
    );
}
