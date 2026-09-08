//! OpencodeGo Zen `/zen/go/v1/usage` defensive parsers (issue #193 P3).
//!
//! Primary shape: `usage.{rolling,weekly,monthly}.{status,percent,resetsAt}`.
//! Compat shape: identical windows but keyed `usage_percent` / `resets_in_seconds`
//! (and flat under `windows`, or flush to the top level). Percent is the used
//! share; remaining is derived as `100 - clamp(percent)` by the UI/cache.

use chrono::{Duration, Utc};
use serde_json::Value;

use super::command_code_parsing::normalize_reset_at;
use super::json_scalars::{truncate_message, value_as_f64, value_as_i64, value_as_string};
use crate::worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage};

#[derive(Debug, Default, Clone)]
pub(crate) struct OpencodeGoUsage {
    pub(crate) rolling: Option<OpencodeGoWindowUsage>,
    pub(crate) weekly: Option<OpencodeGoWindowUsage>,
    pub(crate) monthly: Option<OpencodeGoWindowUsage>,
}

// HTTP-status business errors: 401 = missing/invalid key, 403 = no
// subscription. These are distinct from the transport error fallback.
pub(crate) fn opencode_go_http_business_error(status: u16) -> Option<(Option<String>, String)> {
    match status {
        401 => Some((
            Some("401".to_string()),
            "OpencodeGo key is missing or invalid".to_string(),
        )),
        403 => Some((
            Some("403".to_string()),
            "OpencodeGo subscription is not available".to_string(),
        )),
        _ => None,
    }
}

// Business error embedded in a 2xx payload (e.g. `{"error": "..."}`).
pub(crate) fn opencode_go_business_error(body: &Value) -> Option<(Option<String>, String)> {
    ["error", "message"]
        .into_iter()
        .filter_map(|key| body.get(key).and_then(value_as_string))
        .find(|message| !message.trim().is_empty())
        .map(|message| (None, truncate_message(message)))
}

// Locate the window container: prefer `usage`, then `windows`, then the body
// itself (flat compat shape). Returns None for non-object bodies.
fn windows_object(body: &Value) -> Option<&serde_json::Map<String, Value>> {
    for key in ["usage", "windows"] {
        if let Some(object) = body.get(key).and_then(Value::as_object) {
            return Some(object);
        }
    }
    body.as_object()
}

// A window is usable if it exposes a percent/status/reset; otherwise it is
// dropped (never padded). percent is the used share clamped to 0..=100.
fn parse_window(value: Option<&Value>) -> Option<OpencodeGoWindowUsage> {
    let window = value?.as_object()?;
    let percent = window
        .get("percent")
        .or_else(|| window.get("usage_percent"))
        .or_else(|| window.get("usagePercent"))
        .and_then(value_as_f64)
        .map(|value| value.clamp(0.0, 100.0));
    let status = window
        .get("status")
        .and_then(value_as_i64)
        .and_then(|value| i32::try_from(value).ok());
    let resets_at = window
        .get("resetsAt")
        .or_else(|| window.get("reset_at"))
        .and_then(normalize_reset_at)
        .or_else(|| {
            let seconds = window
                .get("resets_in_seconds")
                .or_else(|| window.get("reset_in_seconds"))
                .or_else(|| window.get("resetsInSeconds"))
                .or_else(|| window.get("resetInSec"))
                .and_then(value_as_i64)?;
            Some(Utc::now() + Duration::seconds(seconds.max(0)))
        });
    if percent.is_none() && status.is_none() && resets_at.is_none() {
        return None;
    }
    Some(OpencodeGoWindowUsage {
        status,
        percent,
        resets_at,
    })
}

pub(crate) fn parse_opencode_go_usage(body: &Value) -> Option<OpencodeGoUsage> {
    let object = windows_object(body)?;
    let rolling = parse_window(object.get("rolling"));
    let weekly = parse_window(object.get("weekly"));
    let monthly = parse_window(object.get("monthly"));
    if rolling.is_none() && weekly.is_none() && monthly.is_none() {
        return None;
    }
    Some(OpencodeGoUsage {
        rolling,
        weekly,
        monthly,
    })
}

// Tightest (lowest) remaining percent across the present OpencodeGo windows,
// where remaining = 100 - clamp(used). Use for cache reservation weighting.
pub(crate) fn opencode_go_remaining_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    [
        &key.opencodego_rolling,
        &key.opencodego_weekly,
        &key.opencodego_monthly,
    ]
    .into_iter()
    .filter_map(|window| {
        window
            .as_ref()
            .and_then(|window| window.percent.map(|used| 100.0 - used))
    })
    .min_by(|a, b| a.total_cmp(b))
    .map(|value| value.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn full_key(
        rolling: Option<f64>,
        weekly: Option<f64>,
        monthly: Option<f64>,
    ) -> TokenPlanKeyUsage {
        let window = |percent: f64| OpencodeGoWindowUsage {
            status: None,
            percent: Some(percent),
            resets_at: None,
        };
        TokenPlanKeyUsage {
            key_id: Uuid::nil(),
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: rolling.map(window),
            opencodego_weekly: weekly.map(window),
            opencodego_monthly: monthly.map(window),
            openrouter_balance: None,
            openrouter_spend: None,
        }
    }

    #[test]
    fn primary_usage_shape_parses_all_windows() {
        let body = json!({
            "usage": {
                "rolling": {"status": "ok", "percent": 12, "resetsAt": "2026-08-22T14:00:00.000Z"},
                "weekly": {"status": "ok", "percent": 34, "resetsAt": "2026-08-22T14:00:00.000Z"},
                "monthly": {"status": "ok", "percent": 56, "resetsAt": "2026-08-22T14:00:00.000Z"}
            }
        });
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert_eq!(usage.rolling.as_ref().unwrap().percent, Some(12.0));
        assert_eq!(usage.weekly.as_ref().unwrap().percent, Some(34.0));
        assert_eq!(usage.monthly.as_ref().unwrap().percent, Some(56.0));
        assert!(usage.rolling.as_ref().unwrap().resets_at.is_some());
    }

    #[test]
    fn compat_usage_percent_and_resets_in_seconds_parse() {
        let body = json!({
            "windows": {
                "rolling": {"usage_percent": 65, "resets_in_seconds": 2520},
                "weekly": {"usage_percent": 30, "resets_in_seconds": 259200},
                "monthly": {"usage_percent": 12, "resets_in_seconds": 1728000}
            }
        });
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert_eq!(usage.rolling.as_ref().unwrap().percent, Some(65.0));
        assert_eq!(usage.monthly.as_ref().unwrap().percent, Some(12.0));
        // resets_in_seconds anchors resets_at in the future.
        let resets = usage.rolling.as_ref().unwrap().resets_at.unwrap();
        assert!(resets > Utc::now());
    }

    #[test]
    fn missing_window_is_dropped_not_padded() {
        let body =
            json!({"usage": {"rolling": {"percent": 10, "resetsAt": "2026-08-22T14:00:00.000Z"}}});
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert!(usage.rolling.is_some());
        assert!(usage.weekly.is_none());
        assert!(usage.monthly.is_none());
    }

    #[test]
    fn flat_body_shape_parses_without_wrapper() {
        let body = json!({"rolling": {"usage_percent": 5}, "weekly": {"usage_percent": 8}});
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert_eq!(usage.rolling.as_ref().unwrap().percent, Some(5.0));
        assert_eq!(usage.weekly.as_ref().unwrap().percent, Some(8.0));
        assert!(usage.monthly.is_none());
    }

    #[test]
    fn percent_is_clamped_and_empty_windows_rejected() {
        let body =
            json!({"usage": {"rolling": {"percent": 130, "resetsAt": "2026-08-22T14:00:00.000Z"}}});
        assert_eq!(
            parse_opencode_go_usage(&body)
                .unwrap()
                .rolling
                .unwrap()
                .percent,
            Some(100.0)
        );
        assert!(parse_opencode_go_usage(&json!({"balance": {"usd": 1.0}})).is_none());
        assert!(parse_opencode_go_usage(&json!([])).is_none());
    }

    #[test]
    fn epoch_seconds_and_millis_fall_back() {
        let secs = json!({"usage": {"weekly": {"percent": 1, "resetsAt": 1_700_000_000_i64}}});
        let ms = json!({"usage": {"weekly": {"percent": 1, "resetsAt": 1_700_000_000_000_i64}}});
        let reset = |body: &Value| {
            parse_opencode_go_usage(body)
                .unwrap()
                .weekly
                .unwrap()
                .resets_at
                .unwrap()
                .timestamp()
        };
        assert_eq!(reset(&secs), 1_700_000_000);
        assert_eq!(reset(&ms), 1_700_000_000);
    }

    #[test]
    fn business_errors_distinguish_status_and_body() {
        assert_eq!(
            opencode_go_http_business_error(401).unwrap().0.as_deref(),
            Some("401")
        );
        assert_eq!(
            opencode_go_http_business_error(403).unwrap().0.as_deref(),
            Some("403")
        );
        assert!(opencode_go_http_business_error(500).is_none());
        assert!(opencode_go_business_error(&json!({"error": "nope"})).is_some());
        assert!(opencode_go_business_error(&json!({"message": "nope"})).is_some());
        assert!(opencode_go_business_error(&json!({"usage": {}})).is_none());
    }

    #[test]
    fn remaining_percent_takes_the_tightest_window() {
        assert_eq!(
            opencode_go_remaining_percent(&full_key(Some(35.0), Some(75.0), Some(90.0))),
            Some(10.0)
        );
        assert_eq!(
            opencode_go_remaining_percent(&full_key(Some(1.0), None, None)),
            Some(99.0)
        );
        assert_eq!(
            opencode_go_remaining_percent(&full_key(None, Some(100.0), None)),
            Some(0.0)
        );
        assert_eq!(
            opencode_go_remaining_percent(&full_key(None, None, None)),
            None
        );
    }

    #[test]
    fn percent_ties_break_toward_the_first_window_with_the_same_remaining() {
        // When two windows yield the same remaining, the min is stable and
        // independent of presentation order.
        assert_eq!(
            opencode_go_remaining_percent(&full_key(Some(30.0), Some(30.0), None)),
            Some(70.0)
        );
        assert_eq!(
            opencode_go_remaining_percent(&full_key(None, Some(30.0), Some(30.0))),
            Some(70.0)
        );
    }

    #[test]
    fn missing_or_empty_window_is_dropped_not_padded_empty() {
        // A window with only a status and no percent/reset is still "present"
        // (it carries a signal) but a fully empty object does not count.
        let body = json!({
            "usage": {
                "rolling": {"status": 1},
                "weekly": {},
                "monthly": {"percent": 5}
            }
        });
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert!(usage.rolling.is_some(), "status-only window is retained");
        assert!(usage.weekly.is_none(), "empty object is dropped");
        assert!(usage.monthly.is_some());
    }

    #[test]
    fn clamp_applies_to_negative_and_oversized_percent_ticks() {
        let body = json!({
            "usage": {
                "rolling": {"percent": -3, "resetsAt": "2026-08-22T14:00:00.000Z"},
                "weekly": {"usagePercent": 140}
            }
        });
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert_eq!(usage.rolling.as_ref().unwrap().percent, Some(0.0));
        assert_eq!(usage.weekly.as_ref().unwrap().percent, Some(100.0));
    }
}
