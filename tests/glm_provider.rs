//! GLM provider external contract (issue #230 P4).
//!
//! The P1 contract is locked by `src/db/types/endpoints::tests::
//! glm_provider_round_trips_as_snake_case`; this file pins the small
//! public surface an external caller can rely on: the standalone
//! `EndpointProvider::Glm` round-trip and the public serde contract
//! of `TokenPlanKeyUsage` (the `glm_five_hour` / `glm_weekly` sections
//! omit when absent). No live key and no network here.

use prompt_ferry::{
    db::EndpointProvider,
    standalone_config,
    worker_admin_types::{GlmWindowUsage, TokenPlanKeyUsage},
};
use serde_json::json;
use uuid::Uuid;

fn glm_window(limit: f64, current_value: f64, percentage: Option<f64>) -> GlmWindowUsage {
    GlmWindowUsage {
        limit,
        current_value,
        remaining: limit - current_value,
        percentage,
        next_reset_at: None,
    }
}

#[test]
fn key_usage_serde_omits_absent_glm_sections() {
    fn key(five_hour: Option<GlmWindowUsage>, weekly: Option<GlmWindowUsage>) -> TokenPlanKeyUsage {
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
            glm_five_hour: five_hour,
            glm_weekly: weekly,
            deepseek_balance: None,
        }
    }
    // A key carrying no GLM sections must not serialize them (the
    // existing `#[serde(skip_serializing_if = "Option::is_none")]`
    // annotations on the GLM fields mean absent == omitted).
    let absent = serde_json::to_value(&key(None, None)).expect("serialize key");
    assert!(absent.get("glm_five_hour").is_none());
    assert!(absent.get("glm_weekly").is_none());
    // A key with both windows serializes each under its field and the
    // 5h `limit` / `current_value` shape round-trips so the admin UI
    // can render the "used / total" subline.
    let present = serde_json::to_value(&key(
        Some(glm_window(50000.0, 1000.0, Some(2.0))),
        Some(glm_window(100.0, 5.0, Some(5.0))),
    ))
    .expect("serialize key");
    assert_eq!(present["glm_five_hour"]["limit"], json!(50000.0));
    assert_eq!(present["glm_five_hour"]["current_value"], json!(1000.0));
    assert_eq!(present["glm_five_hour"]["percentage"], json!(2.0));
    assert_eq!(present["glm_weekly"]["limit"], json!(100.0));
    assert_eq!(present["glm_weekly"]["current_value"], json!(5.0));
    assert_eq!(present["glm_weekly"]["percentage"], json!(5.0));
}

#[test]
fn standalone_provider_round_trips_glm() {
    let serialized = serde_json::to_value(standalone_config::EndpointProvider::Glm)
        .expect("serialize standalone provider");
    assert_eq!(serialized, json!("glm"));
    let deserialized: standalone_config::EndpointProvider =
        serde_json::from_value(json!("glm")).expect("deserialize standalone provider");
    assert_eq!(deserialized, standalone_config::EndpointProvider::Glm);
    // The standalone enum is the source of truth for the 0015
    // migration CHECK; the wire value must be exactly `glm` so
    // existing rows survive an open() that re-applies 0015.
    assert_eq!(EndpointProvider::Glm.as_str(), "glm");
}
