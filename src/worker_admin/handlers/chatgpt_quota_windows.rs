//! Issue #759 P1: strict parsing of the ChatGPT `primary_window` /
//! `secondary_window` payloads.
//!
//! The upstream quota response sends `null`, `{}`, or a partially populated
//! object. Only a genuine object that carries at least one recognized field
//! becomes a [`ChatgptQuotaWindow`]; a `null`, non-object, empty, or
//! unrecognized payload is rejected so the caller never fabricates a window.
//! Values are normalized here — a percentage is kept only when it is finite and
//! inside `0..=100`, a duration only when positive, a countdown only when
//! non-negative — so an invalid value stays unknown instead of being clamped
//! into a fake `0%` or `100%`.

use chrono::DateTime;
use serde_json::Value;

use super::chatgpt_backend::ChatgptQuotaWindow;

/// Fields the parser recognizes on a rate-limit window object. A window payload
/// with none of them is empty/unrecognized and rejected.
const WINDOW_FIELDS: [&str; 4] = [
    "used_percent",
    "limit_window_seconds",
    "reset_after_seconds",
    "reset_at",
];

/// Parse one `primary_window`/`secondary_window` value. `None` for a `null`,
/// non-object, empty, or unrecognized payload; `Some` for a genuine object,
/// with each field individually normalized to `None` when invalid.
pub(super) fn parse_window(value: &Value) -> Option<ChatgptQuotaWindow> {
    let object = value.as_object()?;
    if !WINDOW_FIELDS
        .iter()
        .any(|field| object.contains_key(*field))
    {
        return None;
    }
    let window = ChatgptQuotaWindow {
        used_percent: normalize_percent(object.get("used_percent")),
        limit_window_seconds: normalize_positive_seconds(object.get("limit_window_seconds")),
        reset_after_seconds: normalize_reset_seconds(object.get("reset_after_seconds")),
        reset_at: object
            .get("reset_at")
            .and_then(Value::as_i64)
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
    };
    (window.used_percent.is_some()
        || window.limit_window_seconds.is_some()
        || window.reset_after_seconds.is_some()
        || window.reset_at.is_some())
    .then_some(window)
}

/// A used share is valid only when finite and inside `0..=100`. Anything else
/// (missing, `null`, a string, `NaN`, out of range) is unknown.
fn normalize_percent(value: Option<&Value>) -> Option<f64> {
    let value = value?.as_f64()?;
    (value.is_finite() && (0.0..=100.0).contains(&value)).then_some(value)
}

/// A window duration is valid only when strictly positive.
fn normalize_positive_seconds(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|seconds| *seconds > 0)
}

/// A reset countdown is valid when non-negative (zero means already due).
fn normalize_reset_seconds(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|seconds| *seconds >= 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_null_and_non_object_windows() {
        assert!(parse_window(&Value::Null).is_none());
        assert!(parse_window(&json!(7)).is_none());
        assert!(parse_window(&json!("weekly")).is_none());
        assert!(parse_window(&json!([])).is_none());
    }

    #[test]
    fn rejects_empty_unrecognized_and_malformed_objects() {
        assert!(parse_window(&json!({})).is_none());
        assert!(parse_window(&json!({ "unrelated": true })).is_none());
        assert!(parse_window(&json!({ "used_percent": null })).is_none());
        assert!(parse_window(&json!({ "reset_at": "not a timestamp" })).is_none());
    }

    #[test]
    fn keeps_a_known_window_with_duration_and_countdown() {
        let window = parse_window(&json!({
            "used_percent": 2.0,
            "limit_window_seconds": 604_800,
            "reset_after_seconds": 3_600,
            "reset_at": 1_700_000_000,
        }))
        .expect("valid window");
        assert_eq!(window.used_percent, Some(2.0));
        assert_eq!(window.limit_window_seconds, Some(604_800));
        assert_eq!(window.reset_after_seconds, Some(3_600));
        assert_eq!(window.reset_at, DateTime::from_timestamp(1_700_000_000, 0));
    }

    #[test]
    fn keeps_a_valid_object_with_unknown_percent() {
        // The window exists upstream but reports no usable percentage: it must
        // survive as a window with `used_percent = None`, not be dropped.
        let window = parse_window(&json!({ "limit_window_seconds": 18_000 }))
            .expect("window with unknown percent");
        assert_eq!(window.used_percent, None);
        assert_eq!(window.limit_window_seconds, Some(18_000));
    }

    #[test]
    fn keeps_a_genuine_full_usage_window() {
        // `used_percent = 100` is a real exhausted window and must survive.
        let window = parse_window(&json!({
            "used_percent": 100.0,
            "limit_window_seconds": 604_800,
        }))
        .expect("exhausted window");
        assert_eq!(window.used_percent, Some(100.0));
    }

    #[test]
    fn invalid_values_stay_unknown_instead_of_being_clamped() {
        for payload in [
            json!({ "used_percent": 120.0, "limit_window_seconds": 18_000 }),
            json!({ "used_percent": -1.0, "limit_window_seconds": 18_000 }),
            json!({ "used_percent": "half", "limit_window_seconds": 18_000 }),
        ] {
            let window = parse_window(&payload).expect("recognized field");
            assert_eq!(window.used_percent, None, "payload={payload}");
        }
        // Non-positive duration and negative countdown are dropped, never
        // rewritten to a positive placeholder.
        let window = parse_window(&json!({
            "limit_window_seconds": 0,
            "reset_after_seconds": -5,
            "reset_at": 1_700_000_000,
        }))
        .expect("valid reset anchor");
        assert_eq!(window.limit_window_seconds, None);
        assert_eq!(window.reset_after_seconds, None);
    }
}
