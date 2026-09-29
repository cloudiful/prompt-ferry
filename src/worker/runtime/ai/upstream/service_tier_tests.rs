//! Issue #637 P3: service-tier override behavior tests.
//!
//! The resolved `RouteConfig.service_tier` (target override wins over the
//! endpoint value, `None` means inherit) is applied to supported
//! MiniMax/OpenAI bodies; with no override the caller body/provider
//! default is preserved, and all unrelated providers pass through
//! byte-for-byte. Free-form values are preserved verbatim via the shared
//! [`crate::db::EndpointProvider::service_tier_wire_key`] policy. Shared
//! fixtures live here;
//! core override semantics live in [`override_tests`], protocol and
//! forwarding parity in [`protocol_tests`], so each file stays focused.

mod override_tests;
mod protocol_tests;

use super::*;
use crate::{
    config::NativeApi,
    db::{EndpointProvider, RouteConfig, RouteSelectionReason},
};

fn responses_route(provider: EndpointProvider, tier: Option<&str>) -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: match provider {
            EndpointProvider::OpenAi => "https://api.openai.com/v1".to_string(),
            _ => "https://api.minimaxi.com".to_string(),
        },
        api_key: "test-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: RouteSelectionReason::Default,
        provider,
        service_tier: tier.map(str::to_string),
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
    }
}

fn route_with_protocol(route: &RouteConfig, native_api: NativeApi) -> RouteConfig {
    RouteConfig {
        native_api,
        ..route.clone()
    }
}

#[test]
fn wire_key_is_shared_with_the_provider_capability() {
    // The live transform, the route probe and the compatibility
    // translation share `EndpointProvider::supports_service_tier`; the
    // wire key follows the same bit.
    assert_eq!(
        EndpointProvider::Minimax.service_tier_wire_key(),
        Some("service_tier")
    );
    assert_eq!(
        EndpointProvider::OpenAi.service_tier_wire_key(),
        Some("service_tier")
    );
    for provider in [
        EndpointProvider::Generic,
        EndpointProvider::CommandCode,
        EndpointProvider::OpencodeGo,
        EndpointProvider::OpenRouter,
        EndpointProvider::Glm,
        EndpointProvider::DeepSeek,
    ] {
        assert_eq!(
            provider.service_tier_wire_key(),
            None,
            "{provider:?} must never gain a service-tier wire key"
        );
    }
}
