//! CommandCode alpha defensive parsers (issue #184 P3).

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use super::json_scalars::{truncate_message, value_as_f64, value_as_i64, value_as_string};
use crate::worker_admin_types::{CommandCodeWindowUsage, TokenPlanKeyUsage};

// Monthly pool by plan: Go 10, GOAT 70, Pro 80, Max10x 150, Max20x 300,
// TeamPro 40, Provider 0. Unknown → None (fallback to credits field).
pub(crate) fn plan_monthly_credits(plan_id: &str) -> Option<f64> {
    let compact: String = plan_id
        .trim()
        .to_ascii_lowercase()
        .replace(['_', ' '], "-")
        .chars()
        .filter(|c| *c != '-')
        .collect();
    match compact.as_str() {
        "go" => Some(10.0),
        "goat" => Some(70.0),
        "pro" => Some(80.0),
        "teampro" | "team" => Some(40.0),
        "ultra" => Some(300.0),
        "provider" | "payg" | "payasyougo" => Some(0.0),
        "max10x" | "max10" => Some(150.0),
        "max20x" | "max20" => Some(300.0),
        _ => None,
    }
}

// Business codes even on HTTP 200: non-zero code or success/ok false.
pub(crate) fn command_code_business_error(body: &Value) -> Option<(Option<String>, String)> {
    if let Some(raw) = body.get("code").or_else(|| body.get("status_code"))
        && let Some(number) = value_as_i64(raw)
        && number != 0
    {
        let message = body
            .get("message")
            .and_then(value_as_string)
            .unwrap_or_else(|| format!("CommandCode rejected the credential ({number})"));
        return Some((Some(number.to_string()), truncate_message(message)));
    }
    if body.get("success").and_then(Value::as_bool) == Some(false)
        || body.get("ok").and_then(Value::as_bool) == Some(false)
    {
        let message = body
            .get("message")
            .and_then(value_as_string)
            .unwrap_or_else(|| "CommandCode rejected the credential".to_string());
        return Some((None, truncate_message(message)));
    }
    match body.get("error") {
        Some(Value::String(message)) if !message.trim().is_empty() => {
            Some((None, truncate_message(message.clone())))
        }
        _ => None,
    }
}

// resetAt: ms epoch (live shape) or ISO-8601.
pub(crate) fn normalize_reset_at(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if text.chars().all(|c| c.is_ascii_digit())
            && let Ok(number) = text.parse::<i64>()
        {
            return reset_datetime(number);
        }
        return DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    value_as_i64(value).and_then(reset_datetime)
}

fn reset_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    if timestamp < 0 {
        return None;
    }
    if timestamp >= 1_000_000_000_000 {
        Utc.timestamp_millis_opt(timestamp).single()
    } else {
        Utc.timestamp_opt(timestamp, 0).single()
    }
}

// org.id; None = personal account (org null/missing). Unknown shapes → None.
pub(crate) fn parse_whoami_org_id(body: &Value) -> Option<Option<String>> {
    body.get("org")
        .and_then(Value::as_object)
        .and_then(|org| org.get("login"))
        .and_then(value_as_string)
        .filter(|login| !login.trim().is_empty())
        .or_else(|| {
            body.get("user")?
                .as_object()?
                .get("userName")
                .and_then(value_as_string)
                .filter(|login| !login.trim().is_empty())
        })?;
    match body.get("org") {
        None | Some(Value::Null) => Some(None),
        Some(Value::Object(org)) => Some(
            org.get("id")
                .and_then(value_as_string)
                .filter(|id| !id.trim().is_empty()),
        ),
        _ => Some(None),
    }
}

pub(crate) struct ParsedCredits {
    pub(crate) monthly: Option<f64>,
    pub(crate) purchased: Option<f64>,
    pub(crate) free: Option<f64>,
    pub(crate) premium_monthly: Option<f64>,
    pub(crate) opensource_monthly: Option<f64>,
    pub(crate) windows: Value,
}

pub(crate) fn parse_credits_section(body: &Value) -> Option<ParsedCredits> {
    let credits = body.get("credits")?.as_object()?;
    let monthly = credits.get("monthlyCredits").and_then(value_as_f64);
    let purchased = credits.get("purchasedCredits").and_then(value_as_f64);
    let free = credits.get("freeCredits").and_then(value_as_f64);
    let premium_monthly = credits.get("premiumMonthlyCredits").and_then(value_as_f64);
    let opensource_monthly = credits
        .get("opensourceMonthlyCredits")
        .and_then(value_as_f64);
    if monthly.is_none()
        && purchased.is_none()
        && free.is_none()
        && premium_monthly.is_none()
        && opensource_monthly.is_none()
    {
        return None;
    }
    let windows = body.get("windowLimits").cloned().unwrap_or(Value::Null);
    Some(ParsedCredits {
        monthly,
        purchased,
        free,
        premium_monthly,
        opensource_monthly,
        windows,
    })
}

/// Remaining monthly balance for display. The provider-reported remaining
/// value always wins; the plan table is only a compatibility fallback for
/// responses that omit `monthlyCredits` entirely.
pub(crate) fn effective_monthly_credits(parsed: Option<f64>, plan_id: Option<&str>) -> f64 {
    parsed
        .or_else(|| plan_id.and_then(plan_monthly_credits))
        .unwrap_or(0.0)
}

/// Any explicitly reported monthly pool at or below zero removes the whole
/// CommandCode key from routing (issue #656). The pools are the premium and
/// open-source splits; a missing split (older PAYG response) carries no
/// signal and keeps the window-only behavior.
pub(crate) fn monthly_pools_exhausted(premium: Option<f64>, opensource: Option<f64>) -> bool {
    [premium, opensource]
        .into_iter()
        .flatten()
        .any(|remaining| remaining <= 0.0)
}

/// Effective credits the key can still spend for routing. An exhausted
/// monthly split pool empties the whole key even when another pool still
/// holds credits, so the effective remaining collapses to 0.
pub(crate) fn effective_remaining_credits(
    monthly: f64,
    purchased: f64,
    free: f64,
    premium_monthly: Option<f64>,
    opensource_monthly: Option<f64>,
) -> f64 {
    if monthly_pools_exhausted(premium_monthly, opensource_monthly) {
        0.0
    } else {
        monthly + purchased + free
    }
}

/// A CommandCode key with no effective credits left drops out of routing.
/// Keys without a parsed credits section carry no signal and keep the
/// window-only behavior.
pub(crate) fn command_code_balance_exhausted(key: &TokenPlanKeyUsage) -> bool {
    key.balances
        .as_ref()
        .is_some_and(|balances| balances.remaining_credits <= 0.0)
}

pub(crate) fn parse_subscription_plan(body: &Value) -> Option<Option<String>> {
    let data = body.get("data")?.as_object()?;
    let plan = data.get("planId").and_then(value_as_string);
    let status = data.get("status").and_then(value_as_string);
    if plan.is_none() && status.is_none() {
        return None;
    }
    Some(plan)
}

pub(crate) fn parse_summary_present(body: &Value) -> bool {
    let Some(object) = body.as_object() else {
        return false;
    };
    object.get("totalCost").and_then(value_as_f64).is_some()
        && object.get("totalCount").and_then(value_as_f64).is_some()
}

pub(crate) fn parse_window_entry(
    windows: &Value,
    key: &str,
) -> Option<(f64, f64, Option<DateTime<Utc>>)> {
    let entry = windows.as_object()?.get(key)?.as_object()?;
    let used = entry.get("used").and_then(value_as_f64)?;
    let cap = entry.get("cap").and_then(value_as_f64)?;
    if used == 0.0 && cap == 0.0 {
        return None;
    }
    Some((used, cap, entry.get("resetAt").and_then(normalize_reset_at)))
}

// USD used/cap → used% = used/cap*100, remaining% = 100-used%.
pub(crate) fn build_window_usage(
    used: f64,
    cap: f64,
    reset_at: Option<DateTime<Utc>>,
) -> CommandCodeWindowUsage {
    let (used_percent, remaining_percent) = if cap > 0.0 {
        let used = (used / cap * 100.0).clamp(0.0, 100.0);
        (Some(used), Some((100.0 - used).clamp(0.0, 100.0)))
    } else {
        (None, None)
    };
    CommandCodeWindowUsage {
        used,
        cap,
        used_percent,
        remaining_percent,
        reset_at,
    }
}

/// Tighter of the two USD windows for cache weighting. A key whose effective
/// balance is exhausted reads as fully depleted regardless of windows.
pub(crate) fn command_code_remaining_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    if command_code_balance_exhausted(key) {
        return Some(0.0);
    }
    match (
        key.five_hour
            .as_ref()
            .and_then(|window| window.remaining_percent),
        key.weekly
            .as_ref()
            .and_then(|window| window.remaining_percent),
    ) {
        (Some(five), Some(weekly)) => Some(five.min(weekly).clamp(0.0, 100.0)),
        (Some(remaining), None) | (None, Some(remaining)) => Some(remaining.clamp(0.0, 100.0)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::CommandCodeBalances;
    use serde_json::json;

    use super::super::quota_window_weight::command_code_weight_percent;

    #[test]
    fn whoami_org_null_covers_personal_accounts() {
        let personal = json!({"user":{"userName":"alice"},"org":null});
        let org = json!({"user":{"userName":"alice"},"org":{"id":"org_1","login":"a"}});
        let unknown = json!({"changed":"schema"});
        let blank = json!({"user":{"userName":"  "},"org":{"id":"o","login":"  "}});
        assert_eq!(parse_whoami_org_id(&personal), Some(None));
        assert_eq!(parse_whoami_org_id(&org), Some(Some("org_1".into())));
        assert_eq!(parse_whoami_org_id(&unknown), None);
        assert_eq!(parse_whoami_org_id(&blank), None);
    }

    #[test]
    fn plan_window_and_business_rules_hold() {
        assert_eq!(plan_monthly_credits("go"), Some(10.0));
        assert_eq!(plan_monthly_credits("pro"), Some(80.0));
        assert_eq!(plan_monthly_credits("max-10x"), Some(150.0));
        assert_eq!(plan_monthly_credits("provider"), Some(0.0));
        assert_eq!(plan_monthly_credits("custom"), None);
        let w = build_window_usage(8.0, 16.0, None);
        let got = (w.used_percent, w.remaining_percent);
        assert_eq!(got, (Some(50.0), Some(50.0)));
        assert_eq!(build_window_usage(1.0, 0.0, None).remaining_percent, None);
        let bad = json!({"code":401,"message":"bad key"});
        let ok = json!({"credits":{"monthlyCredits":5}});
        let got = command_code_business_error(&bad).map(|(c, _)| c);
        assert_eq!(got, Some(Some("401".into())));
        assert_eq!(command_code_business_error(&ok), None);
    }

    #[test]
    fn business_error_branches_hold() {
        let a = json!({"success":false,"message":"nope"});
        let b = json!({"ok":false});
        let c = json!({"error":"boom"});
        assert!(command_code_business_error(&a).is_some());
        assert!(command_code_business_error(&b).is_some());
        assert!(command_code_business_error(&c).is_some());
        let d = json!({"success":false});
        let msg = command_code_business_error(&d).expect("branch").1;
        assert_eq!(msg, "CommandCode rejected the credential");
        let blank = json!({"error":"  "});
        assert!(command_code_business_error(&blank).is_none());
    }

    #[test]
    fn remaining_percent_four_arms_and_clamp_hold() {
        fn window(remaining: f64) -> CommandCodeWindowUsage {
            build_window_usage(10.0 - remaining / 10.0, 10.0, None)
        }
        fn key(five: Option<f64>, weekly: Option<f64>) -> TokenPlanKeyUsage {
            TokenPlanKeyUsage {
                key_id: uuid::Uuid::nil(),
                key_label: "k".into(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                model_remains: Vec::new(),
                balances: None,
                five_hour: five.map(window),
                weekly: weekly.map(window),
                opencodego_rolling: None,
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
            }
        }
        fn pct(five: Option<f64>, weekly: Option<f64>) -> Option<f64> {
            command_code_remaining_percent(&key(five, weekly))
        }
        assert_eq!(pct(Some(25.0), Some(75.0)), Some(25.0));
        assert_eq!(pct(Some(50.0), None), Some(50.0));
        assert_eq!(pct(None, Some(75.0)), Some(75.0));
        assert_eq!(pct(None, None), None);
        assert_eq!(pct(Some(120.0), Some(75.0)), Some(75.0));
        assert_eq!(pct(Some(120.0), None), Some(100.0));
    }

    #[test]
    fn reset_and_payg_degrade_hold() {
        let ms = json!(1_700_000_000_000_i64);
        let got = normalize_reset_at(&ms).map(|v| v.timestamp());
        assert_eq!(got, Some(1_700_000_000));
        let iso = json!("2023-11-14T22:13:20.000Z");
        assert!(normalize_reset_at(&iso).is_some());
        let body = json!({"credits":{"monthlyCredits":5}});
        let parsed = parse_credits_section(&body).expect("credits");
        assert_eq!(parse_window_entry(&parsed.windows, "fiveHour"), None);
    }

    #[test]
    fn plan_and_subscription_boundaries_hold() {
        assert_eq!(plan_monthly_credits(" Go "), Some(10.0));
        assert_eq!(plan_monthly_credits("MAX_10X"), Some(150.0));
        assert_eq!(plan_monthly_credits("team-pro"), Some(40.0));
        assert_eq!(plan_monthly_credits("pay-as-you-go"), Some(0.0));
        assert_eq!(plan_monthly_credits(""), None);
        assert_eq!(plan_monthly_credits("custom"), None);
        let with_plan = json!({"data": {"planId": "goat", "status": "active"}});
        assert_eq!(
            parse_subscription_plan(&with_plan),
            Some(Some("goat".into()))
        );
        assert!(parse_subscription_plan(&json!({"data": {}})).is_none());
        assert!(parse_subscription_plan(&json!({})).is_none());
        assert!(parse_summary_present(
            &json!({"totalCost": 1.0, "totalCount": 2})
        ));
        assert!(!parse_summary_present(&json!({"totalCost": 1.0})));
        assert!(!parse_summary_present(&json!([1])));
    }

    #[test]
    fn credits_and_window_boundaries_hold() {
        let body =
            json!({"credits": {"monthlyCredits": "80", "purchasedCredits": 5, "freeCredits": 0}});
        let parsed = parse_credits_section(&body).expect("credits");
        assert_eq!(parsed.monthly, Some(80.0));
        assert_eq!(parsed.purchased, Some(5.0));
        assert!(parse_credits_section(&json!({"credits": {}})).is_none());
        assert!(parse_credits_section(&json!({})).is_none());
        let windows = json!({
            "empty": {"used": 0.0, "cap": 0.0},
            "stringy": {"used": "8", "cap": "16", "resetAt": "2023-11-14T22:13:20.000Z"},
        });
        assert!(parse_window_entry(&windows, "empty").is_none());
        assert!(parse_window_entry(&windows, "missing").is_none());
        let (used, cap, reset) = parse_window_entry(&windows, "stringy").expect("window");
        assert_eq!((used, cap), (8.0, 16.0));
        assert!(reset.is_some());
        assert_eq!(
            build_window_usage(120.0, 100.0, None).used_percent,
            Some(100.0)
        );
        assert_eq!(
            build_window_usage(120.0, 100.0, None).remaining_percent,
            Some(0.0)
        );
        assert_eq!(
            build_window_usage(-5.0, 100.0, None).used_percent,
            Some(0.0)
        );
        assert!(build_window_usage(1.0, 0.0, None).used_percent.is_none());
    }

    #[test]
    fn scalar_and_org_boundaries_hold() {
        assert!(normalize_reset_at(&json!(-1)).is_none());
        assert!(normalize_reset_at(&json!("")).is_none());
        assert!(normalize_reset_at(&json!("not-a-date")).is_none());
        assert!(normalize_reset_at(&json!("1700000000")).is_some());
        assert!(normalize_reset_at(&json!(1_700_000_000_i64)).is_some());
        let no_org = json!({"user": {"userName": "a"}});
        assert_eq!(parse_whoami_org_id(&no_org), Some(None));
        let blank_id = json!({"user": {"userName": "a"}, "org": {"id": "  ", "login": "a"}});
        assert_eq!(parse_whoami_org_id(&blank_id), Some(None));
        let numeric_id = json!({"user": {"userName": "a"}, "org": {"id": 42, "login": "a"}});
        assert_eq!(parse_whoami_org_id(&numeric_id), Some(Some("42".into())));
        assert!(command_code_business_error(&json!({"code": 0})).is_none());
        assert!(
            command_code_business_error(&json!({"status_code": "403", "message": "x"})).is_some()
        );
        assert!(command_code_business_error(&json!({"error": {"detail": "x"}})).is_none());
    }

    fn command_code_key(
        monthly: f64,
        purchased: f64,
        free: f64,
        premium_monthly: Option<f64>,
        opensource_monthly: Option<f64>,
        five_hour: Option<f64>,
        weekly: Option<f64>,
    ) -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id: uuid::Uuid::nil(),
            key_label: "cc".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: Vec::new(),
            balances: Some(CommandCodeBalances {
                monthly_credits: monthly,
                purchased_credits: purchased,
                free_credits: free,
                remaining_credits: effective_remaining_credits(
                    monthly,
                    purchased,
                    free,
                    premium_monthly,
                    opensource_monthly,
                ),
            }),
            five_hour: five_hour
                .map(|percent| build_window_usage(10.0 - percent / 10.0, 10.0, None)),
            weekly: weekly.map(|percent| build_window_usage(10.0 - percent / 10.0, 10.0, None)),
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
        }
    }

    #[test]
    fn exhausted_monthly_split_pool_drops_the_whole_command_code_key() {
        // Exact shape from issue #656: the premium pool is empty while the
        // open-source pool still holds credits, yet the whole key must drop
        // out of routing even though both USD windows read 100%.
        let body = json!({
            "credits": {
                "monthlyCredits": 0.1155473458,
                "purchasedCredits": 0,
                "premiumMonthlyCredits": 0,
                "opensourceMonthlyCredits": 0.1155473458,
            },
            "windowLimits": {
                "fiveHour": {"used": 0, "cap": 14},
                "weekly": {"used": 0, "cap": 35},
            },
        });
        let parsed = parse_credits_section(&body).expect("credits");
        assert_eq!(parsed.monthly, Some(0.1155473458));
        assert_eq!(parsed.premium_monthly, Some(0.0));
        assert_eq!(parsed.opensource_monthly, Some(0.1155473458));
        assert!(monthly_pools_exhausted(
            parsed.premium_monthly,
            parsed.opensource_monthly
        ));
        let key = command_code_key(
            0.1155473458,
            0.0,
            0.0,
            parsed.premium_monthly,
            parsed.opensource_monthly,
            Some(100.0),
            Some(100.0),
        );
        let balances = key.balances.as_ref().expect("balances");
        assert_eq!(balances.monthly_credits, 0.1155473458);
        assert_eq!(balances.remaining_credits, 0.0);
        assert_eq!(command_code_remaining_percent(&key), Some(0.0));
        assert_eq!(command_code_weight_percent(&key), Some(0.0));
    }

    #[test]
    fn positive_monthly_split_pools_keep_the_window_weight() {
        let parsed = parse_credits_section(&json!({
            "credits": {
                "monthlyCredits": 5,
                "premiumMonthlyCredits": 2,
                "opensourceMonthlyCredits": 3,
            }
        }))
        .expect("credits");
        assert!(!monthly_pools_exhausted(
            parsed.premium_monthly,
            parsed.opensource_monthly
        ));
        let key = command_code_key(5.0, 0.0, 0.0, Some(2.0), Some(3.0), Some(40.0), Some(90.0));
        assert_eq!(command_code_remaining_percent(&key), Some(40.0));
        assert_eq!(command_code_weight_percent(&key), Some(40.0));
    }

    #[test]
    fn missing_monthly_split_fields_keep_payg_routing() {
        let parsed = parse_credits_section(&json!({
            "credits": {"monthlyCredits": 0.0, "purchasedCredits": 10.0}
        }))
        .expect("credits");
        assert_eq!(parsed.premium_monthly, None);
        assert_eq!(parsed.opensource_monthly, None);
        assert!(!monthly_pools_exhausted(
            parsed.premium_monthly,
            parsed.opensource_monthly
        ));
        let key = command_code_key(0.0, 10.0, 0.0, None, None, Some(60.0), None);
        assert_eq!(
            key.balances.as_ref().expect("balances").remaining_credits,
            10.0
        );
        assert_eq!(command_code_remaining_percent(&key), Some(60.0));
        assert_eq!(command_code_weight_percent(&key), Some(60.0));
        let no_windows = command_code_key(0.0, 10.0, 0.0, None, None, None, None);
        assert_eq!(command_code_remaining_percent(&no_windows), None);
    }

    #[test]
    fn reported_monthly_balance_wins_over_the_plan_cap() {
        assert_eq!(
            effective_monthly_credits(Some(0.1155), Some("goat")),
            0.1155
        );
        assert_eq!(effective_monthly_credits(None, Some("goat")), 70.0);
        assert_eq!(effective_monthly_credits(None, Some("provider")), 0.0);
        assert_eq!(effective_monthly_credits(None, None), 0.0);
    }
}
