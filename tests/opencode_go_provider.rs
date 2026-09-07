//! OpencodeGo provider external contracts (issue #193 P4).
//!
//! These lock only what an external caller can rely on: the provider enum
//! wire string, the standalone provider round-trip, and the public serde
//! contract of `TokenPlanKeyUsage` (the `opencodego_*` windows omit when
//! absent). Percent-math and parser behavior are exercised by the crate's
//! own unit tests in `src/worker_admin/opencode_go_parsing.rs`, which call
//! the production functions directly; no live key and no network here.

use prompt_ferry::{
    db::EndpointProvider,
    standalone_config,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage},
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn provider_wire_values_match_minimax_convention() {
    assert_eq!(EndpointProvider::OpencodeGo.as_str(), "opencode_go");
    assert_eq!(
        EndpointProvider::from_str("opencode_go"),
        EndpointProvider::OpencodeGo
    );
    assert_eq!(
        EndpointProvider::from_optional(Some("opencode_go")),
        EndpointProvider::OpencodeGo
    );
    assert_eq!(EndpointProvider::Minimax.as_str(), "minimax");
    assert_eq!(EndpointProvider::CommandCode.as_str(), "command_code");
    assert_eq!(EndpointProvider::Generic.as_str(), "generic");
    assert_eq!(EndpointProvider::default(), EndpointProvider::Generic);
    // Serde uses snake_case, matching the admin API contract.
    let serialized = serde_json::to_value(EndpointProvider::OpencodeGo).expect("serialize provider");
    assert_eq!(serialized, json!("opencode_go"));
    let deserialized: EndpointProvider =
        serde_json::from_value(json!("opencode_go")).expect("deserialize provider");
    assert_eq!(deserialized, EndpointProvider::OpencodeGo);
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
fn standalone_provider_round_trips_opencode_go() {
    let serialized = serde_json::to_value(standalone_config::EndpointProvider::OpencodeGo)
        .expect("serialize standalone provider");
    assert_eq!(serialized, json!("opencode_go"));
    let deserialized: standalone_config::EndpointProvider =
        serde_json::from_value(json!("opencode_go")).expect("deserialize standalone provider");
    assert_eq!(
        deserialized,
        standalone_config::EndpointProvider::OpencodeGo
    );
    assert!(serde_json::from_value::<standalone_config::EndpointProvider>(
        json!("legacy-unknown")
    )
    .is_err());
}

fn opencode_go_window(percent: f64) -> OpencodeGoWindowUsage {
    OpencodeGoWindowUsage {
        status: None,
        percent: Some(percent),
        resets_at: None,
    }
}

#[test]
fn key_usage_serde_omits_absent_opencode_go_sections() {
    fn key(
        rolling: Option<f64>,
        weekly: Option<f64>,
        monthly: Option<f64>,
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
            opencodego_rolling: rolling.map(opencode_go_window),
            opencodego_weekly: weekly.map(opencode_go_window),
            opencodego_monthly: monthly.map(opencode_go_window),
        }
    }
    // A key carrying no opencode_go windows must not serialize them.
    let absent = serde_json::to_value(&key(None, None, None)).expect("serialize key");
    assert!(absent.get("opencodego_rolling").is_none());
    assert!(absent.get("opencodego_weekly").is_none());
    assert!(absent.get("opencodego_monthly").is_none());
    // A key with all three percent windows serializes each under its field.
    let present = serde_json::to_value(&key(Some(12.0), Some(34.0), Some(56.0)))
        .expect("serialize key");
    assert_eq!(present["opencodego_rolling"]["percent"], json!(12.0));
    assert_eq!(present["opencodego_weekly"]["percent"], json!(34.0));
    assert_eq!(present["opencodego_monthly"]["percent"], json!(56.0));
}
