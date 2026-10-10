//! Issue #757 P1: shared prompt-cache test fixtures.

use std::borrow::Cow;

use super::super::*;
use crate::{
    config::NativeApi,
    db::{CompactMode, EndpointProvider, RouteConfig, RouteSelectionReason},
};

/// An OpenRouter Responses route whose configured model resolves to Anthropic.
pub(super) fn openrouter_route() -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://openrouter.ai/api/v1".to_string(),
        api_key: "test-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Responses,
        upstream_model: Some("anthropic/claude-haiku-5.5".to_string()),
        route_selection_reason: RouteSelectionReason::Default,
        provider: EndpointProvider::OpenRouter,
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
    }
}

pub(super) fn targeted_route(
    provider: EndpointProvider,
    native_api: NativeApi,
    upstream_model: Option<&str>,
) -> RouteConfig {
    RouteConfig {
        provider,
        native_api,
        upstream_model: upstream_model.map(str::to_string),
        ..openrouter_route()
    }
}

pub(super) fn forwarded_bytes<'a>(
    route: &RouteConfig,
    path: &str,
    body: &'a [u8],
) -> Cow<'a, [u8]> {
    apply_openrouter_prompt_cache(route, path, Cow::Borrowed(body))
}

/// A no-op must forward the caller's bytes untouched, still borrowed.
pub(super) fn assert_unchanged(route: &RouteConfig, path: &str, body: &[u8]) {
    let out = forwarded_bytes(route, path, body);
    assert!(matches!(&out, Cow::Borrowed(_)), "{body:?}");
    assert_eq!(out.as_ref(), body, "{body:?}");
}
