//! OpenRouter provider external contracts (issue #203 P4).
//!
//! These lock only what an external caller can rely on: the provider enum
//! wire string, the standalone provider round-trip, and the public serde
//! contract of `TokenPlanKeyUsage` (the `openrouter_*` sections omit when
//! absent). Percent-math and parser behavior are exercised by the crate's
//! own unit tests in `src/worker_admin/openrouter_parsing.rs` and
//! `src/worker_admin/token_plan_cache.rs`, which call the production
//! functions directly; no live key and no network here.

use prompt_ferry::{
    db::EndpointProvider,
    standalone_config,
    worker_admin_types::{OpenRouterBalance, OpenRouterSpend, TokenPlanKeyUsage},
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn provider_wire_values_match_minimax_convention() {
    assert_eq!(EndpointProvider::OpenRouter.as_str(), "openrouter");
    assert_eq!(
        EndpointProvider::from_str("openrouter"),
        EndpointProvider::OpenRouter
    );
    assert_eq!(
        EndpointProvider::from_optional(Some("openrouter")),
        EndpointProvider::OpenRouter
    );
    assert_eq!(EndpointProvider::Minimax.as_str(), "minimax");
    assert_eq!(EndpointProvider::CommandCode.as_str(), "command_code");
    assert_eq!(EndpointProvider::OpencodeGo.as_str(), "opencode_go");
    assert_eq!(EndpointProvider::Generic.as_str(), "generic");
    assert_eq!(EndpointProvider::default(), EndpointProvider::Generic);
    // Serde uses the single-token `openrouter`, matching the admin API contract.
    let serialized =
        serde_json::to_value(EndpointProvider::OpenRouter).expect("serialize provider");
    assert_eq!(serialized, json!("openrouter"));
    let deserialized: EndpointProvider =
        serde_json::from_value(json!("openrouter")).expect("deserialize provider");
    assert_eq!(deserialized, EndpointProvider::OpenRouter);
    // Unknown providers keep the legacy generic fallback.
    assert_eq!(
        EndpointProvider::from_str("legacy-unknown"),
        EndpointProvider::Generic
    );
    assert_eq!(
        EndpointProvider::from_optional(None),
        EndpointProvider::Generic
    );
}

#[test]
fn standalone_provider_round_trips_openrouter() {
    let serialized = serde_json::to_value(standalone_config::EndpointProvider::OpenRouter)
        .expect("serialize standalone provider");
    assert_eq!(serialized, json!("openrouter"));
    let deserialized: standalone_config::EndpointProvider =
        serde_json::from_value(json!("openrouter")).expect("deserialize standalone provider");
    assert_eq!(
        deserialized,
        standalone_config::EndpointProvider::OpenRouter
    );
    assert!(
        serde_json::from_value::<standalone_config::EndpointProvider>(json!("legacy-unknown"))
            .is_err()
    );
}

fn openrouter_balance(limit: Option<f64>, remaining: Option<f64>) -> OpenRouterBalance {
    OpenRouterBalance {
        limit,
        limit_remaining: remaining,
        limit_reset: None,
        is_free_tier: false,
        total_credits: None,
        total_usage: None,
    }
}

fn openrouter_spend() -> OpenRouterSpend {
    OpenRouterSpend {
        usage: 25.5,
        daily: 1.5,
        weekly: 5.25,
        monthly: 12.0,
    }
}

#[test]
fn key_usage_serde_omits_absent_openrouter_sections() {
    fn key(
        balance: Option<OpenRouterBalance>,
        spend: Option<OpenRouterSpend>,
    ) -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id: Uuid::nil(),
            key_label: "k".to_string(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: balance,
            openrouter_spend: spend,
        }
    }
    // A key carrying no OpenRouter sections must not serialize them.
    let absent = serde_json::to_value(&key(None, None)).expect("serialize key");
    assert!(absent.get("openrouter_balance").is_none());
    assert!(absent.get("openrouter_spend").is_none());
    // A key with balance + spend serializes each under its field.
    let present = serde_json::to_value(&key(
        Some(openrouter_balance(Some(100.0), Some(74.5))),
        Some(openrouter_spend()),
    ))
    .expect("serialize key");
    assert_eq!(present["openrouter_balance"]["limit"], json!(100.0));
    assert_eq!(
        present["openrouter_balance"]["limit_remaining"],
        json!(74.5)
    );
    assert_eq!(present["openrouter_spend"]["usage"], json!(25.5));
    assert_eq!(present["openrouter_spend"]["monthly"], json!(12.0));
}
