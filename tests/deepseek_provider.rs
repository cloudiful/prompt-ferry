//! DeepSeek provider external contracts (issue #287 P0).
//!
//! These lock only what an external caller can rely on: the provider enum wire
//! string, the standalone provider round-trip, and the public serde contract of
//! `TokenPlanKeyUsage` (the `deepseek_balance` section omits when absent).
//! Balance parsing, HTTP business-error mapping and percent/weight math are
//! exercised by the crate's own unit tests; no live key and no network here.

use prompt_ferry::{
    db::EndpointProvider,
    standalone_config,
    worker_admin_types::{DeepSeekBalance, TokenPlanKeyUsage},
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn provider_wire_values_match_minimax_convention() {
    assert_eq!(EndpointProvider::DeepSeek.as_str(), "deepseek");
    assert_eq!(
        EndpointProvider::from_str("deepseek"),
        EndpointProvider::DeepSeek
    );
    assert_eq!(
        EndpointProvider::from_optional(Some("deepseek")),
        EndpointProvider::DeepSeek
    );
    assert_eq!(EndpointProvider::default(), EndpointProvider::Generic);
    // Serde uses the single-token `deepseek`, matching the admin API contract.
    let serialized = serde_json::to_value(EndpointProvider::DeepSeek).expect("serialize provider");
    assert_eq!(serialized, json!("deepseek"));
    let deserialized: EndpointProvider =
        serde_json::from_value(json!("deepseek")).expect("deserialize provider");
    assert_eq!(deserialized, EndpointProvider::DeepSeek);
    // Unknown providers keep the legacy generic fallback.
    assert_eq!(
        EndpointProvider::from_str("legacy-unknown"),
        EndpointProvider::Generic
    );
}

#[test]
fn standalone_provider_round_trips_deepseek() {
    let serialized = serde_json::to_value(standalone_config::EndpointProvider::DeepSeek)
        .expect("serialize standalone provider");
    assert_eq!(serialized, json!("deepseek"));
    let deserialized: standalone_config::EndpointProvider =
        serde_json::from_value(json!("deepseek")).expect("deserialize standalone provider");
    assert_eq!(deserialized, standalone_config::EndpointProvider::DeepSeek);
    assert!(
        serde_json::from_value::<standalone_config::EndpointProvider>(json!("legacy-unknown"))
            .is_err()
    );
    assert!(
        serde_json::from_value::<standalone_config::EndpointProvider>(json!("deep_seek")).is_err()
    );
}

fn balance() -> DeepSeekBalance {
    DeepSeekBalance {
        is_available: true,
        currency: "CNY".to_string(),
        total_balance: 110.0,
        granted_balance: 10.0,
        topped_up_balance: 100.0,
    }
}

#[test]
fn key_usage_serde_omits_absent_deepseek_balance() {
    fn key(balance: Option<DeepSeekBalance>) -> TokenPlanKeyUsage {
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
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: balance,
        }
    }
    // A key carrying no DeepSeek section must not serialize it.
    let absent = serde_json::to_value(&key(None)).expect("serialize key");
    assert!(absent.get("deepseek_balance").is_none());
    // A key with balance serializes each field under its section.
    let present = serde_json::to_value(&key(Some(balance()))).expect("serialize key");
    assert_eq!(present["deepseek_balance"]["is_available"], json!(true));
    assert_eq!(present["deepseek_balance"]["currency"], json!("CNY"));
    assert_eq!(present["deepseek_balance"]["total_balance"], json!(110.0));
    assert_eq!(present["deepseek_balance"]["granted_balance"], json!(10.0));
    assert_eq!(
        present["deepseek_balance"]["topped_up_balance"],
        json!(100.0)
    );
}
