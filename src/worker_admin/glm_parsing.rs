//! GLM/Zhipu Coding Plan defensive parsers (issue #230 P2).
//!
//! `GET {base_origin}/api/monitor/usage/quota/limit` (Bearer key) returns
//! either a JSON envelope carrying `data.limits[]` or a plain-text
//! business error (a non-Coding-Plan key gets a bare `Unauthorized` body).
//! Limit rows are tagged `TOKENS_LIMIT` (legacy) or `CREDIT_LIMIT` (new
//! Zhipu billing for GLM Max/Lite); `TIME_LIMIT` rows describe MCP/Add-on
//! service quotas and are ignored. The 5-hour and weekly windows are
//! identified by the `nextResetTime` delta from "now" (5h reset lands
//! within 0..=12h, weekly within 1..=14d) with the `unit/number` pair
//! as a tie-breaker.

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use super::json_scalars::{truncate_message, value_as_f64, value_as_i64, value_as_string};
use crate::worker_admin_types::{GlmWindowUsage, TokenPlanKeyUsage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlmWindowKind {
    FiveHour,
    Weekly,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct GlmQuotaUsage {
    pub(crate) five_hour: Option<GlmWindowUsage>,
    pub(crate) weekly: Option<GlmWindowUsage>,
}

/// JSON envelope business error. Zhipu returns `{code, msg}` even on
/// HTTP 200; `code != 0` is always a rejection.
pub(crate) fn glm_envelope_error(body: &Value) -> Option<(Option<String>, String)> {
    if let Some(code) = body.get("code").and_then(value_as_i64) {
        if code == 0 {
            return None;
        }
        let message = body
            .get("msg")
            .or_else(|| body.get("message"))
            .and_then(value_as_string)
            .unwrap_or_else(|| format!("GLM rejected the credential ({code})"));
        return Some((Some(code.to_string()), truncate_message(message)));
    }
    let flag_false = body.get("success").and_then(Value::as_bool) == Some(false)
        || body.get("ok").and_then(Value::as_bool) == Some(false);
    if flag_false {
        let message = body
            .get("msg")
            .or_else(|| body.get("message"))
            .and_then(value_as_string)
            .unwrap_or_else(|| "GLM rejected the credential".to_string());
        return Some((None, truncate_message(message)));
    }
    match body.get("error") {
        Some(Value::String(message)) if !message.trim().is_empty() => {
            Some((None, truncate_message(message.clone())))
        }
        _ => None,
    }
}

/// HTTP-status business errors: 401 = missing/invalid Coding Plan key,
/// 403 = key valid but not on a Coding Plan, 429 = quota/rate limit.
pub(crate) fn glm_http_business_error(status: u16) -> Option<(Option<String>, String)> {
    match status {
        401 => Some((
            Some("401".to_string()),
            "GLM API key is missing or invalid".to_string(),
        )),
        403 => Some((
            Some("403".to_string()),
            "GLM API key is not enrolled in a Coding Plan".to_string(),
        )),
        429 => Some((
            Some("429".to_string()),
            "GLM Coding Plan rate limit reached (retry later)".to_string(),
        )),
        _ => None,
    }
}

/// `nextResetTime` accepts epoch seconds, epoch milliseconds, or ISO-8601.
pub(crate) fn glm_next_reset(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.chars().all(|c| c.is_ascii_digit())
            && let Ok(number) = trimmed.parse::<i64>()
        {
            return glm_epoch_to_datetime(number);
        }
        return DateTime::parse_from_rfc3339(trimmed)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    value_as_i64(value).and_then(glm_epoch_to_datetime)
}

fn glm_epoch_to_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    if timestamp < 0 {
        return None;
    }
    if timestamp >= 1_000_000_000_000 {
        Utc.timestamp_millis_opt(timestamp).single()
    } else {
        Utc.timestamp_opt(timestamp, 0).single()
    }
}

// Reset-time delta wins when both signals are present; the `unit/number`
// pair is the documented Zhipu fallback (5h: `unit<=12h, number=1`;
// weekly: `unit>=24h, number>=1`).
fn classify_window(row: &Value, now: DateTime<Utc>) -> GlmWindowKind {
    if let Some(reset) = row.get("nextResetTime").and_then(glm_next_reset) {
        let hours = reset.signed_duration_since(now).num_minutes() as f64 / 60.0;
        if (0.0..=12.0).contains(&hours) {
            return GlmWindowKind::FiveHour;
        }
        if (12.0..=14.0 * 24.0).contains(&hours) {
            return GlmWindowKind::Weekly;
        }
    }
    if let (Some(unit), Some(number)) = (
        row.get("unit").and_then(value_as_i64),
        row.get("number").and_then(value_as_i64),
    ) {
        if (1..=12).contains(&unit) && number == 1 {
            return GlmWindowKind::FiveHour;
        }
        if unit >= 24 && number >= 1 {
            return GlmWindowKind::Weekly;
        }
    }
    // No signal: caller drops the row, but classify as 5h so the shorter
    // window wins if the payload carries only one ambiguous row.
    GlmWindowKind::FiveHour
}

fn parse_window_row(row: &Value, now: DateTime<Utc>) -> Option<(GlmWindowKind, GlmWindowUsage)> {
    let kind = classify_window(row, now);
    let limit = row.get("limit").and_then(value_as_f64).unwrap_or(0.0);
    let current_value = row
        .get("currentValue")
        .and_then(value_as_f64)
        .unwrap_or(0.0);
    let remaining = row.get("remaining").and_then(value_as_f64).unwrap_or(0.0);
    let percentage = derive_percentage(
        row.get("percentage").and_then(value_as_f64),
        current_value,
        limit,
    );
    let next_reset_at = row.get("nextResetTime").and_then(glm_next_reset);
    Some((
        kind,
        GlmWindowUsage {
            limit,
            current_value,
            remaining,
            percentage,
            next_reset_at,
        },
    ))
}

fn derive_percentage(reported: Option<f64>, current_value: f64, limit: f64) -> Option<f64> {
    if let Some(value) = reported
        && value.is_finite()
    {
        return Some(value.clamp(0.0, 100.0));
    }
    if limit > 0.0 && current_value.is_finite() {
        return Some((current_value / limit * 100.0).clamp(0.0, 100.0));
    }
    None
}

/// Top-level parser. Walks `data.limits[]`, drops `TIME_LIMIT` rows and
/// unresolvable rows, then surfaces `None` for empty / malformed bodies
/// so the fetcher can report the honest failure. `now` is injection-
/// friendly so tests can pin the reset-time delta.
pub(crate) fn parse_glm_quota(body: &Value, now: DateTime<Utc>) -> Option<GlmQuotaUsage> {
    let data = body.get("data")?.as_object()?;
    let limits = data.get("limits")?.as_array()?;
    if limits.is_empty() {
        return None;
    }
    let mut usage = GlmQuotaUsage::default();
    for row in limits {
        let Some(object) = row.as_object() else {
            continue;
        };
        if !is_quota_row_kind(object) {
            continue;
        }
        if let Some((kind, window)) = parse_window_row(&Value::Object(object.clone()), now) {
            match kind {
                GlmWindowKind::FiveHour if usage.five_hour.is_none() => {
                    usage.five_hour = Some(window);
                }
                GlmWindowKind::Weekly if usage.weekly.is_none() => {
                    usage.weekly = Some(window);
                }
                _ => {}
            }
        }
    }
    if usage.five_hour.is_none() && usage.weekly.is_none() {
        return None;
    }
    Some(usage)
}

fn is_quota_row_kind(object: &serde_json::Map<String, Value>) -> bool {
    let Some(kind) = object.get("type").and_then(value_as_string) else {
        // Legacy payloads without `type` are still treated as quota rows.
        return true;
    };
    matches!(kind.as_str(), "TOKENS_LIMIT" | "CREDIT_LIMIT")
}

/// Tighter (lowest remaining) of the GLM 5-hour and weekly windows for
/// cache reservation weighting. None when neither window carries a
/// `percentage` (degraded → no quota signal).
pub(crate) fn glm_remaining_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    [&key.glm_five_hour, &key.glm_weekly]
        .into_iter()
        .flatten()
        .filter_map(|window| {
            window
                .percentage
                .map(|used| (100.0 - used).clamp(0.0, 100.0))
        })
        .min_by(|a, b| a.total_cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn five_hour_reset() -> i64 {
        // now + 5h in epoch seconds
        let now = chrono::Utc::now().timestamp();
        now + 5 * 3600
    }

    fn weekly_reset() -> i64 {
        let now = chrono::Utc::now().timestamp();
        now + 7 * 24 * 3600
    }

    #[test]
    fn parses_tokens_and_credit_limits_with_reset_delta() {
        // 5h TOKENS_LIMIT (legacy) + weekly CREDIT_LIMIT (new Zhipu
        // billing) must round-trip into the typed windows.
        let body = json!({
            "code": 0,
            "msg": "success",
            "data": {"limits": [
                {"type": "TOKENS_LIMIT", "unit": 5, "number": 1,
                 "currentValue": 1000.0, "limit": 50000.0, "remaining": 49000.0,
                 "percentage": 2.0, "nextResetTime": five_hour_reset()},
                {"type": "CREDIT_LIMIT", "unit": 3, "number": 7,
                 "currentValue": 5.0, "limit": 100.0, "remaining": 95.0,
                 "percentage": 5.0, "nextResetTime": weekly_reset()}
            ]}
        });
        let usage = parse_glm_quota(&body, Utc::now()).expect("quota");
        let five = usage.five_hour.expect("5h window");
        assert_eq!(five.limit, 50000.0);
        assert_eq!(five.current_value, 1000.0);
        assert_eq!(five.percentage, Some(2.0));
        let weekly = usage.weekly.expect("weekly window");
        assert_eq!(weekly.limit, 100.0);
        assert_eq!(weekly.percentage, Some(5.0));
    }

    #[test]
    fn filters_time_limit_and_accepts_legacy_without_type() {
        // TIME_LIMIT rows describe MCP/Add-on service quotas; the parser
        // must drop them. A legacy payload without a `type` field is still
        // treated as a quota row so older deployments do not silently
        // degrade.
        let body = json!({
            "code": 0,
            "msg": "success",
            "data": {"limits": [
                {"type": "TOKENS_LIMIT", "limit": 50000, "currentValue": 100, "remaining": 49900, "nextResetTime": five_hour_reset()},
                {"type": "TIME_LIMIT", "limit": 0, "currentValue": 0, "remaining": 0, "nextResetTime": five_hour_reset() + 86_400}
            ]}
        });
        let usage = parse_glm_quota(&body, Utc::now()).expect("quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_none());
        let legacy = json!({
            "code": 0,
            "data": {"limits": [{
                "unit": 5, "number": 1,
                "limit": 50000.0, "currentValue": 0.0, "remaining": 50000.0
            }]}
        });
        let usage = parse_glm_quota(&legacy, Utc::now()).expect("legacy quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_none());
    }

    #[test]
    fn empty_limits_and_missing_data_reject() {
        assert!(parse_glm_quota(&json!({"code": 0, "data": {"limits": []}}), Utc::now()).is_none());
        assert!(parse_glm_quota(&json!({"code": 0, "data": {}}), Utc::now()).is_none());
        assert!(parse_glm_quota(&json!({"code": 0}), Utc::now()).is_none());
        assert!(parse_glm_quota(&json!({}), Utc::now()).is_none());
    }

    #[test]
    fn missing_fields_zero_remaining_and_derive_percentage() {
        // `remaining` absent and `percentage` missing: remaining stays 0
        // and percentage is derived from currentValue/limit*100. A
        // reported percentage > 100 clamps to 100; a zero limit with
        // missing percentage yields None.
        let body = json!({
            "code": 0,
            "data": {"limits": [{
                "type": "TOKENS_LIMIT",
                "limit": 100.0, "currentValue": 25.0,
                "nextResetTime": five_hour_reset()
            }]}
        });
        let five = parse_glm_quota(&body, Utc::now())
            .unwrap()
            .five_hour
            .expect("5h");
        assert_eq!(five.remaining, 0.0);
        assert_eq!(five.percentage, Some(25.0));
        let clamped = parse_glm_quota(
            &json!({
                "code": 0,
                "data": {"limits": [{
                    "type": "CREDIT_LIMIT",
                    "limit": 100.0, "currentValue": 0.0, "percentage": 250.0,
                    "nextResetTime": weekly_reset()
                }]}
            }),
            Utc::now(),
        )
        .unwrap()
        .weekly
        .unwrap()
        .percentage;
        assert_eq!(clamped, Some(100.0));
        let zero = parse_glm_quota(
            &json!({
                "code": 0,
                "data": {"limits": [{
                    "type": "CREDIT_LIMIT",
                    "limit": 0.0, "currentValue": 0.0,
                    "nextResetTime": weekly_reset()
                }]}
            }),
            Utc::now(),
        )
        .unwrap()
        .weekly
        .unwrap()
        .percentage;
        assert!(zero.is_none());
    }

    #[test]
    fn next_reset_supports_epoch_seconds_and_milliseconds() {
        let iso = json!("2026-08-22T14:00:00.000Z");
        assert!(glm_next_reset(&json!(five_hour_reset())).is_some());
        assert!(glm_next_reset(&json!((five_hour_reset() as i64) * 1000)).is_some());
        assert!(glm_next_reset(&iso).is_some());
        // Zero is the Unix epoch (handled by `classify_window` as a
        // sentinel for "no reset"); negative timestamps are rejected.
        assert!(glm_next_reset(&json!(0)).is_some());
        assert!(glm_next_reset(&json!(-1)).is_none());
        assert!(glm_next_reset(&json!("not-a-date")).is_none());
        assert!(glm_next_reset(&json!("")).is_none());
    }

    #[test]
    fn http_business_error_distinguishes_statuses() {
        for (status, code) in [(401, "401"), (403, "403"), (429, "429")] {
            assert_eq!(
                glm_http_business_error(status).unwrap().0.as_deref(),
                Some(code)
            );
        }
        assert!(glm_http_business_error(500).is_none());
    }
}
