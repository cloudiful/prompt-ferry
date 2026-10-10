//! Issue #757 P1: automatic prompt-cache directive behavior tests.
//!
//! Unit coverage of the injection gate and its boundaries; the builder/pipeline
//! regression lives in [`integration`] and shared fixtures in [`support`].

mod integration;
mod support;

use std::borrow::Cow;

use serde_json::{Value, json};

use self::support::{assert_unchanged, forwarded_bytes, openrouter_route, targeted_route};
use super::*;
use crate::{config::NativeApi, db::EndpointProvider};

#[test]
fn routed_anthropic_alias_receives_the_root_ephemeral_directive() {
    // The executor alias resolves through the route to an Anthropic model; the
    // caller's body model is not what decides.
    let route = openrouter_route();
    let body = br#"{"model":"executor","input":[{"role":"user","content":"hi"}]}"#;
    let out = apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Borrowed(body));
    assert!(matches!(&out, Cow::Owned(_)));
    let value: Value = serde_json::from_slice(out.as_ref()).unwrap();
    assert_eq!(value["cache_control"], json!({"type": "ephemeral"}));
    // Default TTL only, and unrelated fields survive semantically.
    assert!(value["cache_control"].get("ttl").is_none());
    assert_eq!(value["model"], "executor");
    assert_eq!(value["input"][0]["content"], "hi");
}

#[test]
fn body_model_is_the_fallback_only_without_a_route_model() {
    let untargeted = targeted_route(EndpointProvider::OpenRouter, NativeApi::Responses, None);
    let anthropic = forwarded_bytes(
        &untargeted,
        CACHE_PATH,
        br#"{"model":"anthropic/claude-haiku-5.5"}"#,
    );
    let value: Value = serde_json::from_slice(anthropic.as_ref()).unwrap();
    assert_eq!(value["cache_control"], json!({"type": "ephemeral"}));

    for body in [
        br#"{"model":"openai/gpt-5"}"#.as_slice(),
        br#"{"model":"Anthropic/claude-haiku-5.5"}"#.as_slice(),
        br#"{"model":"anthropic/"}"#.as_slice(),
        br#"{"model":""}"#.as_slice(),
        br#"{"model":42}"#.as_slice(),
        br#"{"input":[]}"#.as_slice(),
    ] {
        assert_unchanged(&untargeted, CACHE_PATH, body);
    }

    // A configured non-Anthropic route model is authoritative: the Anthropic
    // body model does not re-enable the directive.
    let authoritative = targeted_route(
        EndpointProvider::OpenRouter,
        NativeApi::Responses,
        Some("openai/gpt-5"),
    );
    assert_unchanged(
        &authoritative,
        CACHE_PATH,
        br#"{"model":"anthropic/claude-haiku-5.5"}"#,
    );
}

#[test]
fn caller_cache_intent_at_any_depth_blocks_injection() {
    let route = openrouter_route();
    for body in [
        br#"{"model":"m","cache_control":{"type":"ephemeral"}}"#.as_slice(),
        br#"{"model":"m","cache_control":null}"#.as_slice(),
        br#"{"model":"m","prompt_cache_breakpoint":{"type":"ephemeral"}}"#.as_slice(),
        br#"{"model":"m","prompt_cache_breakpoint":false}"#.as_slice(),
        br#"{"model":"m","input":[{"cache_control":null}]}"#.as_slice(),
        br#"{"model":"m","input":[{"content":[{"type":"text","cache_control":{"type":"ephemeral"}}]}]}"#.as_slice(),
        br#"{"model":"m","tools":[{"function":{"parameters":{"x":1},"prompt_cache_breakpoint":null}}}]}"#.as_slice(),
        br#"{"model":"m","x":[{"y":[{"z":null}]}],"prompt_cache_breakpoint":{"type":"ephemeral"}}"#.as_slice(),
    ] {
        assert_unchanged(&route, CACHE_PATH, body);
    }
}

#[test]
fn non_targets_are_forwarded_byte_identical() {
    let body = br#"{"model":"executor","input":[]}"#;
    for provider in [
        EndpointProvider::Generic,
        EndpointProvider::OpenAi,
        EndpointProvider::Minimax,
        EndpointProvider::Glm,
        EndpointProvider::DeepSeek,
        EndpointProvider::CommandCode,
        EndpointProvider::OpencodeGo,
    ] {
        assert_unchanged(
            &targeted_route(provider, NativeApi::Responses, None),
            CACHE_PATH,
            body,
        );
    }
    for native_api in [
        NativeApi::Chat,
        NativeApi::AnthropicMessages,
        NativeApi::Realtime,
    ] {
        assert_unchanged(
            &targeted_route(EndpointProvider::OpenRouter, native_api, None),
            CACHE_PATH,
            body,
        );
    }
    for path in [
        "/v1/responses/compact",
        "/v1/chat/completions",
        "/v1/messages",
        "/V1/Responses",
        "/v1/responses/",
        "",
    ] {
        assert_unchanged(
            &targeted_route(EndpointProvider::OpenRouter, NativeApi::Responses, None),
            path,
            body,
        );
    }
    // Malformed and non-object bodies never reach the injection.
    for body in [
        b"not-json".as_slice(),
        b"".as_slice(),
        b"[{\"model\":\"anthropic/m\"}]".as_slice(),
        b"\"anthropic/m\"".as_slice(),
        b"null".as_slice(),
        b"{\"model\":\"anthropic/m\",}".as_slice(),
    ] {
        assert_unchanged(
            &targeted_route(EndpointProvider::OpenRouter, NativeApi::Responses, None),
            CACHE_PATH,
            body,
        );
    }
}

#[test]
fn an_unscannable_body_is_forwarded_untouched() {
    let route = targeted_route(EndpointProvider::OpenRouter, NativeApi::Responses, None);
    let items: Vec<String> = (0..=MAX_SCANNED_NODES).map(|i| i.to_string()).collect();
    let body = format!(r#"{{"model":"anthropic/m","input":[{}]}}"#, items.join(","));
    let out =
        apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Owned(body.clone().into_bytes()));
    // An owned body stays owned and byte-identical when the scan gives up.
    assert!(matches!(&out, Cow::Owned(_)));
    assert_eq!(out.as_ref(), body.as_bytes());

    // A reviewable body of the same shape is still injected.
    let small = format!(
        r#"{{"model":"anthropic/m","input":[{}]}}"#,
        (0..8).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    );
    let out = apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Borrowed(small.as_bytes()));
    let value: Value = serde_json::from_slice(out.as_ref()).unwrap();
    assert_eq!(value["cache_control"], json!({"type": "ephemeral"}));
}

#[test]
fn a_second_application_is_a_no_op() {
    let route = openrouter_route();
    let body = br#"{"model":"executor","input":[]}"#;
    let first = apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Borrowed(body));
    let first_bytes = first.to_vec();
    let second = apply_openrouter_prompt_cache(&route, CACHE_PATH, first);
    assert!(matches!(&second, Cow::Owned(_)));
    assert_eq!(second.as_ref(), first_bytes.as_slice());
}

#[test]
fn no_op_bodies_keep_their_bytes_and_cow_variant() {
    // Whitespace and formatting of an untouched body are never rewritten.
    let body = b"{\n  \"model\": \"openai/gpt-5\",\n  \"input\": []\n}\n";
    let route = targeted_route(EndpointProvider::OpenRouter, NativeApi::Responses, None);
    let out = apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Borrowed(body));
    assert!(matches!(&out, Cow::Borrowed(_)));
    assert_eq!(out.as_ref(), body);

    let out = apply_openrouter_prompt_cache(&route, CACHE_PATH, Cow::Owned(body.to_vec()));
    assert!(matches!(&out, Cow::Owned(_)));
    assert_eq!(out.as_ref(), body);
}
