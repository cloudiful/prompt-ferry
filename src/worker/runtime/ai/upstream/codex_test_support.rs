//! Issue #599 R2f.1: shared fixtures for the Codex subscription request tests.
//!
//! `codex_auth_tests` and `codex_header_tests` share the route fixture, the
//! claim-carrying JWT builder, and the Codex request wrapper, so both modules
//! stay focused on their assertions and under the file-size budget.

use super::*;
use crate::{
    config::NativeApi,
    db::{RouteConfig, RouteSelectionReason},
};

pub(super) fn openai_responses_route() -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 7,
        model_route_rule_id: None,
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: "platform-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: RouteSelectionReason::Default,
        provider: crate::db::EndpointProvider::OpenAi,
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_downgrade_enabled: false,
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
    }
}

pub(super) fn jwt_with_claims(claims: serde_json::Value) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

pub(super) fn codex_request(
    body: &[u8],
    headers: &[(String, String)],
    auth: &CodexAuth,
) -> reqwest::Request {
    build_codex_upstream_request(
        &Client::new(),
        &Method::POST,
        "https://chatgpt.com/backend-api/codex/responses",
        &openai_responses_route(),
        "/v1/responses",
        &PreparedRequestBody::BufferedBytes(body.to_vec()),
        auth,
        headers,
    )
    .build()
    .unwrap()
}
