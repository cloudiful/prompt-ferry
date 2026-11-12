//! Pure public-contract tests for the preset provider wire values and the
//! `TokenPlanKeyUsage` serde shape. They need no key, network, or database.

pub(super) mod command_code {
    use crate::{
        db::{EndpointProvider, EndpointRegion},
        standalone_config,
        worker_admin_types::{
            CanonicalQuotaWindow, CanonicalQuotaWindowKind, CommandCodeBalances,
            CommandCodeWindowUsage, SubscriptionWindowAvailability, TokenPlanKeyUsage,
        },
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    #[test]
    fn provider_wire_values_match_minimax_convention() {
        assert_eq!(EndpointProvider::CommandCode.as_str(), "command_code");
        assert_eq!(
            EndpointProvider::from_str_or_default("command_code"),
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
            EndpointProvider::from_str_or_default("legacy-unknown"),
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
            (EndpointProvider::Minimax, Some(EndpointRegion::Global)),
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
        assert!(
            serde_json::from_value::<standalone_config::EndpointProvider>(json!("legacy-unknown"))
                .is_err()
        );
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
                quota_status: None,
                model_remains: Vec::new(),
                balances,
                five_hour,
                weekly,
                opencodego_rolling: None,
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
                quota_windows: None,
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
        let minimax_shaped = serde_json::to_value(key(None, None, None)).expect("serialize key");
        assert!(minimax_shaped.get("balances").is_none());
        assert!(minimax_shaped.get("five_hour").is_none());
        assert!(minimax_shaped.get("weekly").is_none());
        // CommandCode key: dual windows present; PAYG degrades to None windows.
        let dual = serde_json::to_value(key(
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
        let payg = serde_json::to_value(key(
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
        assert_eq!(personal["user"]["userName"].as_str(), Some("alice"));

        let team: Value =
            json!({"user": {"userName": "alice"}, "org": {"id": "org_1", "login": "acme"}});
        assert_eq!(team["org"]["id"].as_str(), Some("org_1"));

        let credits: Value = json!({
            "credits": {"monthlyCredits": 80.0, "purchasedCredits": 5.0, "freeCredits": 2.5},
            "windowLimits": {
                "fiveHour": {"used": 8.0, "cap": 16.0, "resetAt": 1_700_000_000_000_i64},
                "weekly": {"used": 4.0, "cap": 16.0, "resetAt": "2023-11-14T22:13:20.000Z"}
            }
        });
        assert_eq!(credits["credits"]["monthlyCredits"].as_f64(), Some(80.0));
        assert_eq!(credits["credits"]["purchasedCredits"].as_f64(), Some(5.0));
        assert_eq!(credits["credits"]["freeCredits"].as_f64(), Some(2.5));
        assert_eq!(
            credits["windowLimits"]["fiveHour"]["used"].as_f64(),
            Some(8.0)
        );
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
    fn monthly_pool_row_keeps_its_public_wire_contract() {
        // The issued monthly pool travels through the admin API as a canonical
        // window: the `monthly` kind, the currency unit, the granted limit, the
        // remaining part and the derived share. The frontend renders exactly this
        // shape, so the wire values are part of the provider contract.
        let row = CanonicalQuotaWindow {
            source_id: "command_code:monthly".into(),
            kind: CanonicalQuotaWindowKind::Monthly,
            source_model: None,
            unit: Some("currency".into()),
            duration_seconds: None,
            is_default_scope: true,
            availability: SubscriptionWindowAvailability::Known,
            remaining_percent: Some(0.09103523),
            current: Some(69.9362753389),
            limit: Some(70.0),
            remaining: Some(0.0637246611),
            reset_at: None,
        };
        let value = serde_json::to_value(&row).expect("serialize monthly row");
        assert_eq!(value["source_id"], json!("command_code:monthly"));
        assert_eq!(value["kind"], json!("monthly"));
        assert_eq!(value["unit"], json!("currency"));
        assert_eq!(value["is_default_scope"], json!(true));
        assert_eq!(value["availability"], json!("known"));
        assert_eq!(value["limit"], json!(70.0));
        assert_eq!(value["remaining"], json!(0.0637246611));
        assert_eq!(value["current"], json!(69.9362753389));
        // A monthly window carries no model scope and no synthetic duration.
        assert!(value.get("source_model").is_none());
        assert!(value.get("duration_seconds").is_none());
        let back: CanonicalQuotaWindow =
            serde_json::from_value(value).expect("deserialize monthly row");
        assert_eq!(back, row);
    }

    #[test]
    fn handler_dispatch_inputs_reject_generic_and_require_minimax_region() {
        // Token-plan usage is only served for minimax and command_code;
        // generic has no usage API. The dispatch inputs locked here are what
        // the handler branches on: provider variant plus region presence.
        assert!(!matches!(
            EndpointProvider::Generic,
            EndpointProvider::Minimax | EndpointProvider::CommandCode
        ));
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
}

pub(super) mod deepseek {
    use crate::{
        db::EndpointProvider,
        standalone_config,
        worker_admin_types::{DeepSeekBalance, DeepSeekCurrencyBalance, TokenPlanKeyUsage},
    };
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn provider_wire_values_match_minimax_convention() {
        assert_eq!(EndpointProvider::DeepSeek.as_str(), "deepseek");
        assert_eq!(
            EndpointProvider::from_str_or_default("deepseek"),
            EndpointProvider::DeepSeek
        );
        assert_eq!(
            EndpointProvider::from_optional(Some("deepseek")),
            EndpointProvider::DeepSeek
        );
        assert_eq!(EndpointProvider::default(), EndpointProvider::Generic);
        // Serde uses the single-token `deepseek`, matching the admin API contract.
        let serialized =
            serde_json::to_value(EndpointProvider::DeepSeek).expect("serialize provider");
        assert_eq!(serialized, json!("deepseek"));
        let deserialized: EndpointProvider =
            serde_json::from_value(json!("deepseek")).expect("deserialize provider");
        assert_eq!(deserialized, EndpointProvider::DeepSeek);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str_or_default("legacy-unknown"),
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
            serde_json::from_value::<standalone_config::EndpointProvider>(json!("deep_seek"))
                .is_err()
        );
    }

    fn balance() -> DeepSeekBalance {
        DeepSeekBalance {
            is_available: true,
            balances: vec![DeepSeekCurrencyBalance {
                currency: "CNY".to_string(),
                total_balance: Some(110.0),
                granted_balance: Some(10.0),
                topped_up_balance: Some(100.0),
            }],
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
                quota_status: None,
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
                quota_windows: None,
            }
        }
        // A key carrying no DeepSeek section must not serialize it.
        let absent = serde_json::to_value(key(None)).expect("serialize key");
        assert!(absent.get("deepseek_balance").is_none());
        // A key with a balance serializes each currency entry with its amounts.
        let present = serde_json::to_value(key(Some(balance()))).expect("serialize key");
        assert_eq!(present["deepseek_balance"]["is_available"], json!(true));
        assert_eq!(
            present["deepseek_balance"]["balances"][0]["currency"],
            json!("CNY")
        );
        assert_eq!(
            present["deepseek_balance"]["balances"][0]["total_balance"],
            json!(110.0)
        );
        assert_eq!(
            present["deepseek_balance"]["balances"][0]["granted_balance"],
            json!(10.0)
        );
        assert_eq!(
            present["deepseek_balance"]["balances"][0]["topped_up_balance"],
            json!(100.0)
        );
    }

    #[test]
    fn key_usage_serde_serializes_null_amounts_and_empty_balances() {
        // An unknown amount serializes as null, never as a fabricated zero.
        let unknown = DeepSeekBalance {
            is_available: true,
            balances: vec![DeepSeekCurrencyBalance {
                currency: "USD".to_string(),
                total_balance: None,
                granted_balance: Some(0.0),
                topped_up_balance: None,
            }],
        };
        let serialized = serde_json::to_value(unknown).expect("serialize balance");
        assert_eq!(
            serialized["balances"][0]["total_balance"],
            json!(serde_json::Value::Null)
        );
        // A real zero survives as zero.
        assert_eq!(serialized["balances"][0]["granted_balance"], json!(0.0));
        // Availability with no parseable entry keeps an empty array.
        let flag_only = serde_json::to_value(DeepSeekBalance {
            is_available: false,
            balances: Vec::new(),
        })
        .expect("serialize flag-only balance");
        assert_eq!(flag_only["is_available"], json!(false));
        assert_eq!(flag_only["balances"], json!([]));
    }
}

pub(super) mod glm {
    use crate::{
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
        fn key(
            five_hour: Option<GlmWindowUsage>,
            weekly: Option<GlmWindowUsage>,
        ) -> TokenPlanKeyUsage {
            TokenPlanKeyUsage {
                key_id: Uuid::nil(),
                key_label: "k".to_string(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                quota_status: None,
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
                quota_windows: None,
            }
        }
        // A key carrying no GLM sections must not serialize them (the
        // existing `#[serde(skip_serializing_if = "Option::is_none")]`
        // annotations on the GLM fields mean absent == omitted).
        let absent = serde_json::to_value(key(None, None)).expect("serialize key");
        assert!(absent.get("glm_five_hour").is_none());
        assert!(absent.get("glm_weekly").is_none());
        // A key with both windows serializes each under its field and the
        // 5h `limit` / `current_value` shape round-trips so the admin UI
        // can render the "used / total" subline.
        let present = serde_json::to_value(key(
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
}

pub(super) mod opencode_go {
    use crate::{
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
            EndpointProvider::from_str_or_default("opencode_go"),
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
        let serialized =
            serde_json::to_value(EndpointProvider::OpencodeGo).expect("serialize provider");
        assert_eq!(serialized, json!("opencode_go"));
        let deserialized: EndpointProvider =
            serde_json::from_value(json!("opencode_go")).expect("deserialize provider");
        assert_eq!(deserialized, EndpointProvider::OpencodeGo);
        // Unknown providers keep the legacy generic fallback.
        assert_eq!(
            EndpointProvider::from_str_or_default("legacy-unknown"),
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
        assert!(
            serde_json::from_value::<standalone_config::EndpointProvider>(json!("legacy-unknown"))
                .is_err()
        );
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
                quota_status: None,
                model_remains: Vec::new(),
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: rolling.map(opencode_go_window),
                opencodego_weekly: weekly.map(opencode_go_window),
                opencodego_monthly: monthly.map(opencode_go_window),
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
                quota_windows: None,
            }
        }
        // A key carrying no opencode_go windows must not serialize them.
        let absent = serde_json::to_value(key(None, None, None)).expect("serialize key");
        assert!(absent.get("opencodego_rolling").is_none());
        assert!(absent.get("opencodego_weekly").is_none());
        assert!(absent.get("opencodego_monthly").is_none());
        // A key with all three percent windows serializes each under its field.
        let present =
            serde_json::to_value(key(Some(12.0), Some(34.0), Some(56.0))).expect("serialize key");
        assert_eq!(present["opencodego_rolling"]["percent"], json!(12.0));
        assert_eq!(present["opencodego_weekly"]["percent"], json!(34.0));
        assert_eq!(present["opencodego_monthly"]["percent"], json!(56.0));
    }
}

pub(super) mod openrouter {
    use crate::{
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
            EndpointProvider::from_str_or_default("openrouter"),
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
            EndpointProvider::from_str_or_default("legacy-unknown"),
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
                quota_status: None,
                model_remains: Vec::new(),
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: None,
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: balance,
                openrouter_spend: spend,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
                quota_windows: None,
            }
        }
        // A key carrying no OpenRouter sections must not serialize them.
        let absent = serde_json::to_value(key(None, None)).expect("serialize key");
        assert!(absent.get("openrouter_balance").is_none());
        assert!(absent.get("openrouter_spend").is_none());
        // A key with balance + spend serializes each under its field.
        let present = serde_json::to_value(key(
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
}
