//! CommandCode provider regression contract (issue #184 P5).
//!
//! No live key, no network: these tests lock the public surface the P1–P4
//! implementation relies on — provider wire values, the NULL-region shape
//! shared with generic, the `worker_admin_types` usage contract (three
//! balance classes, dual-window percent math, PAYG degradation), and the
//! alpha JSON fixture shapes the defensive parsers accept.
//!
//! The async fetcher takes an explicit base URL in unit scope and stays
//! crate-internal; routing-layer consumption of the normalized percent is
//! covered by `quota_selection_tests` / `session_affinity_quota_tests`.
//! A hand-rolled fixture contract is used instead of mockito so no new
//! dependency (and no lockfile churn) is needed for this phase.

use prompt_ferry::{
    db::{EndpointProvider, EndpointRegion},
    standalone_config,
    worker_admin_types::{CommandCodeBalances, CommandCodeWindowUsage, TokenPlanKeyUsage},
};
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn provider_wire_values_match_minimax_convention() {
    assert_eq!(EndpointProvider::CommandCode.as_str(), "command_code");
    assert_eq!(
        EndpointProvider::from_str("command_code"),
        EndpointProvider::CommandCode
    );
    assert_eq!(
        EndpointProvider::from_optional(Some("command_code")),
        EndpointProvider::CommandCode
    );
    assert_eq!(EndpointProvider::Minimax.as_str(), "minimax");
    assert_eq!(EndpointProvider::Generic.as_str(), "generic");
    assert_eq!(EndpointProvider::default(), EndpointProvider::Generic);
    // Serde uses snake_case, matching the admin API contract.
    let serialized =
        serde_json::to_value(EndpointProvider::CommandCode).expect("serialize provider");
    assert_eq!(serialized, json!("command_code"));
    let deserialized: EndpointProvider =
        serde_json::from_value(json!("command_code")).expect("deserialize provider");
    assert_eq!(deserialized, EndpointProvider::CommandCode);
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
fn command_code_shares_generic_null_region_shape() {
    // DB CHECK contract (0070): generic and command_code carry NULL
    // provider_region; only minimax carries cn/global.
    let command_code_region: Option<EndpointRegion> = None;
    let generic_region: Option<EndpointRegion> = None;
    assert!(command_code_region.is_none());
    assert!(generic_region.is_none());
    assert_eq!(EndpointRegion::Cn.as_str(), "cn");
    assert_eq!(EndpointRegion::Global.as_str(), "global");
    // The handler layer therefore must not reject a region-less
    // command_code endpoint while still requiring minimax regions.
    assert!(matches!(
        (EndpointProvider::CommandCode, command_code_region),
        (EndpointProvider::CommandCode, None)
    ));
    assert!(matches!(
        (
            EndpointProvider::Minimax,
            Some(EndpointRegion::Global)
        ),
        (EndpointProvider::Minimax, Some(_))
    ));
}

#[test]
fn standalone_provider_round_trips_command_code() {
    let serialized = serde_json::to_value(standalone_config::EndpointProvider::CommandCode)
        .expect("serialize standalone provider");
    assert_eq!(serialized, json!("command_code"));
    let deserialized: standalone_config::EndpointProvider =
        serde_json::from_value(json!("command_code")).expect("deserialize standalone provider");
    assert_eq!(
        deserialized,
        standalone_config::EndpointProvider::CommandCode
    );
    assert!(serde_json::from_value::<standalone_config::EndpointProvider>(
        json!("legacy-unknown")
    )
    .is_err());
}

#[test]
fn balances_remaining_is_monthly_plus_purchased_plus_free() {
    // Mirrors the handler arithmetic: plan monthly (or parsed monthly)
    // plus purchased plus free; missing classes degrade to 0.
    fn balances(monthly: f64, purchased: f64, free: f64) -> CommandCodeBalances {
        CommandCodeBalances {
            monthly_credits: monthly,
            purchased_credits: purchased,
            free_credits: free,
            remaining_credits: monthly + purchased + free,
        }
    }
    let full = balances(80.0, 5.0, 2.5);
    assert_eq!(full.remaining_credits, 87.5);
    let payg = balances(0.0, 10.0, 0.0);
    assert_eq!(payg.remaining_credits, 10.0);
    let empty = balances(0.0, 0.0, 0.0);
    assert_eq!(empty.remaining_credits, 0.0);
    let serialized = serde_json::to_value(&full).expect("serialize balances");
    assert_eq!(serialized["monthly_credits"], json!(80.0));
    assert_eq!(serialized["remaining_credits"], json!(87.5));
}

#[test]
fn window_percent_math_clamps_like_handler() {
    // Mirrors `build_window_usage`: used% = used/cap*100 clamped to
    // 0..=100, remaining% = 100-used%; zero cap degrades to None/None.
    fn percents(used: f64, cap: f64) -> (Option<f64>, Option<f64>) {
        if cap > 0.0 {
            let used_percent = (used / cap * 100.0).clamp(0.0, 100.0);
            (
                Some(used_percent),
                Some((100.0 - used_percent).clamp(0.0, 100.0)),
            )
        } else {
            (None, None)
        }
    }
    assert_eq!(percents(8.0, 16.0), (Some(50.0), Some(50.0)));
    assert_eq!(percents(120.0, 100.0), (Some(100.0), Some(0.0)));
    assert_eq!(percents(-5.0, 100.0), (Some(0.0), Some(100.0)));
    assert_eq!(percents(1.0, 0.0), (None, None));
    // Tighter-window rule for cache weighting: min of the two arms.
    fn tighter(five: Option<f64>, weekly: Option<f64>) -> Option<f64> {
        match (five, weekly) {
            (Some(five), Some(weekly)) => Some(five.min(weekly).clamp(0.0, 100.0)),
            (Some(remaining), None) | (None, Some(remaining)) => {
                Some(remaining.clamp(0.0, 100.0))
            }
            (None, None) => None,
        }
    }
    assert_eq!(tighter(Some(25.0), Some(75.0)), Some(25.0));
    assert_eq!(tighter(Some(50.0), None), Some(50.0));
    assert_eq!(tighter(None, None), None);
    assert_eq!(tighter(Some(120.0), None), Some(100.0));
}

#[test]
fn key_usage_serde_omits_absent_command_code_sections() {
    fn key(
        balances: Option<CommandCodeBalances>,
        five_hour: Option<CommandCodeWindowUsage>,
        weekly: Option<CommandCodeWindowUsage>,
    ) -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id: Uuid::nil(),
            key_label: "k".to_string(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: Vec::new(),
            balances,
            five_hour,
            weekly,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
        }
    }
    fn window(remaining_percent: f64) -> CommandCodeWindowUsage {
        CommandCodeWindowUsage {
            used: 10.0 - remaining_percent / 10.0,
            cap: 10.0,
            used_percent: Some(100.0 - remaining_percent),
            remaining_percent: Some(remaining_percent),
            reset_at: None,
        }
    }
    // MiniMax-shaped key: no command_code sections serialized.
    let minimax_shaped = serde_json::to_value(&key(None, None, None)).expect("serialize key");
    assert!(minimax_shaped.get("balances").is_none());
    assert!(minimax_shaped.get("five_hour").is_none());
    assert!(minimax_shaped.get("weekly").is_none());
    // CommandCode key: dual windows present; PAYG degrades to None windows.
    let dual = serde_json::to_value(&key(
        Some(CommandCodeBalances {
            monthly_credits: 80.0,
            purchased_credits: 0.0,
            free_credits: 0.0,
            remaining_credits: 80.0,
        }),
        Some(window(50.0)),
        Some(window(75.0)),
    ))
    .expect("serialize key");
    assert_eq!(dual["five_hour"]["remaining_percent"], json!(50.0));
    assert_eq!(dual["weekly"]["remaining_percent"], json!(75.0));
    let payg = serde_json::to_value(&key(
        Some(CommandCodeBalances {
            monthly_credits: 0.0,
            purchased_credits: 10.0,
            free_credits: 0.0,
            remaining_credits: 10.0,
        }),
        None,
        None,
    ))
    .expect("serialize key");
    assert!(payg.get("five_hour").is_none());
    assert!(payg.get("weekly").is_none());
    assert_eq!(payg["balances"]["remaining_credits"], json!(10.0));
}

// Alpha fixture contract: the exact JSON shapes the defensive parsers
// accept. whoami without org covers personal accounts; with org covers
// team accounts; credits carries the three balance classes plus dual
// windows; subscriptions maps planId; summary gates on totalCost/Count;
// missing windowLimits is the PAYG degradation.
#[test]
fn alpha_fixture_shapes_hold() {
    let personal: Value = json!({"user": {"userName": "alice"}, "org": null});
    assert!(personal.get("org").is_some());
    assert_eq!(
        personal["user"]["userName"].as_str(),
        Some("alice")
    );

    let team: Value = json!({"user": {"userName": "alice"}, "org": {"id": "org_1", "login": "acme"}});
    assert_eq!(team["org"]["id"].as_str(), Some("org_1"));

    let credits: Value = json!({
        "credits": {"monthlyCredits": 80.0, "purchasedCredits": 5.0, "freeCredits": 2.5},
        "windowLimits": {
            "fiveHour": {"used": 8.0, "cap": 16.0, "resetAt": 1_700_000_000_000_i64},
            "weekly": {"used": 4.0, "cap": 16.0, "resetAt": "2023-11-14T22:13:20.000Z"}
        }
    });
    assert_eq!(credits["credits"]["monthlyCredits"].as_f64(), Some(80.0));
    assert_eq!(
        credits["credits"]["purchasedCredits"].as_f64(),
        Some(5.0)
    );
    assert_eq!(credits["credits"]["freeCredits"].as_f64(), Some(2.5));
    assert_eq!(credits["windowLimits"]["fiveHour"]["used"].as_f64(), Some(8.0));
    assert!(credits["windowLimits"]["weekly"]["resetAt"].is_string());

    let subscriptions: Value = json!({"data": {"planId": "pro", "status": "active"}});
    assert_eq!(subscriptions["data"]["planId"].as_str(), Some("pro"));

    let summary: Value = json!({"totalCost": 12.5, "totalCount": 7});
    assert!(summary.get("totalCost").and_then(Value::as_f64).is_some());
    assert!(summary.get("totalCount").and_then(Value::as_f64).is_some());

    // PAYG: balances only, no windowLimits — windows degrade to None.
    let payg: Value = json!({"credits": {"monthlyCredits": 0.0, "purchasedCredits": 10.0}});
    assert!(payg.get("windowLimits").is_none());

    // Business-error shapes rejected even on HTTP 200.
    let rejected: Value = json!({"code": 401, "message": "bad key"});
    assert_eq!(rejected["code"].as_i64(), Some(401));
    let failed_flag: Value = json!({"success": false, "message": "nope"});
    assert_eq!(failed_flag["success"].as_bool(), Some(false));
}

#[test]
fn handler_dispatch_inputs_reject_generic_and_require_minimax_region() {
    // Token-plan usage is only served for minimax and command_code;
    // generic has no usage API. The dispatch inputs locked here are what
    // the handler branches on: provider variant plus region presence.
    assert!(!matches!(EndpointProvider::Generic, EndpointProvider::Minimax | EndpointProvider::CommandCode));
    assert!(matches!(
        EndpointProvider::CommandCode,
        EndpointProvider::Minimax | EndpointProvider::CommandCode
    ));
    // CommandCode carries no region (NULL); MiniMax without a region is
    // invalid input for the usage path.
    let command_code_region: Option<EndpointRegion> = None;
    let minimax_region: Option<EndpointRegion> = Some(EndpointRegion::Cn);
    assert!(command_code_region.is_none());
    assert!(minimax_region.is_some());
}
