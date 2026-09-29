//! Issue #637/#644: service-tier override behavior tests.
//!
//! The resolved `RouteConfig.service_tier` (target override wins over the
//! endpoint value, `None` means inherit) is injected as a best-effort
//! top-level `service_tier` passthrough on every provider's HTTP JSON
//! protocols; with no override the caller body/provider default is preserved,
//! and Realtime (WebSocket frames) plus non-JSON bodies pass through
//! byte-for-byte. Free-form values are preserved verbatim via the shared
//! [`crate::db::SERVICE_TIER_WIRE_KEY`] policy. Shared fixtures live here;
//! core override semantics live in [`override_tests`], protocol and
//! forwarding parity in [`protocol_tests`], so each file stays focused.

mod override_tests;
mod protocol_tests;

use super::*;
use crate::{
    config::NativeApi,
    db::{EndpointProvider, RouteConfig, RouteSelectionReason},
};

const ALL_PROVIDERS: [EndpointProvider; 8] = [
    EndpointProvider::Generic,
    EndpointProvider::Minimax,
    EndpointProvider::CommandCode,
    EndpointProvider::OpencodeGo,
    EndpointProvider::OpenRouter,
    EndpointProvider::Glm,
    EndpointProvider::DeepSeek,
    EndpointProvider::OpenAi,
];

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
fn configured_override_is_provider_agnostic_while_caller_bit_stays_scoped() {
    // Issue #644: the configured endpoint/target override injects on every
    // provider's HTTP JSON protocols (the gate takes no provider), while the
    // issue #637 caller-compatibility bit is unchanged and provider-scoped.
    for native_api in [
        NativeApi::Chat,
        NativeApi::Responses,
        NativeApi::AnthropicMessages,
        NativeApi::Auto,
    ] {
        assert!(
            crate::db::supports_service_tier_for(native_api),
            "{native_api:?}"
        );
    }
    assert!(!crate::db::supports_service_tier_for(NativeApi::Realtime));
    assert_eq!(crate::db::SERVICE_TIER_WIRE_KEY, "service_tier");

    for provider in [EndpointProvider::Minimax, EndpointProvider::OpenAi] {
        assert!(provider.supports_service_tier(), "{provider:?}");
    }
    for provider in [
        EndpointProvider::Generic,
        EndpointProvider::CommandCode,
        EndpointProvider::OpencodeGo,
        EndpointProvider::OpenRouter,
        EndpointProvider::Glm,
        EndpointProvider::DeepSeek,
    ] {
        assert!(!provider.supports_service_tier(), "{provider:?}");
    }
}
