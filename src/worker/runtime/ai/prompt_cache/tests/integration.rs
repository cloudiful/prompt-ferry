//! Issue #757 P1: builder/pipeline regression for the prepared-path gate.
//!
//! The directive is decided in the shared body pipeline, so these tests drive
//! `build_upstream_request` and assert that the prepared path — not a
//! reconstructed constant — reaches the helper.

use serde_json::Value;

use super::super::*;
use super::support::{openrouter_route, targeted_route};
use crate::{
    config::NativeApi, db::EndpointProvider, upstream_adapter::PreparedRequestBody,
    worker::runtime::ai::upstream::build_upstream_request,
};

fn forwarded_body(route: &RouteConfig, path: &str, body: &[u8]) -> Value {
    let request = build_upstream_request(
        &reqwest::Client::new(),
        &reqwest::Method::POST,
        "https://openrouter.ai/api/v1/responses",
        route,
        path,
        &PreparedRequestBody::BufferedBytes(body.to_vec()),
        &[],
        None,
    )
    .build()
    .unwrap();
    let bytes = request
        .body()
        .and_then(|body| body.as_bytes())
        .expect("upstream body bytes");
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn the_prepared_path_decides_whether_the_builder_injects() {
    let route = openrouter_route();
    let body = br#"{"model":"executor","input":[]}"#;
    for (path, injected) in [
        ("/v1/responses", true),
        ("/v1/responses/compact", false),
        ("/v1/chat/completions", false),
        ("/v1/messages", false),
    ] {
        for prepared in [
            PreparedRequestBody::BufferedBytes(body.to_vec()),
            PreparedRequestBody::PassthroughStream(body.to_vec()),
        ] {
            let request = build_upstream_request(
                &reqwest::Client::new(),
                &reqwest::Method::POST,
                "https://openrouter.ai/api/v1/responses",
                &route,
                path,
                &prepared,
                &[],
                None,
            )
            .build()
            .unwrap();
            let bytes = request
                .body()
                .and_then(|body| body.as_bytes())
                .expect("upstream body bytes");
            let value: Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                value.get("cache_control").is_some(),
                injected,
                "path {path} body {prepared:?}"
            );
            assert_eq!(value["model"], "executor", "path {path}");
        }
    }
}

#[test]
fn a_non_openrouter_route_stays_untouched_on_the_target_path() {
    let value = forwarded_body(
        &targeted_route(EndpointProvider::Generic, NativeApi::Responses, None),
        CACHE_PATH,
        br#"{"model":"executor","input":[]}"#,
    );
    assert!(value.get("cache_control").is_none());
    assert_eq!(value["model"], "executor");
}
