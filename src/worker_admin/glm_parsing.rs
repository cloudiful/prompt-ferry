//! GLM/Zhipu Coding Plan defensive parsers (issue #230 P2; window
//! identity corrected in issue #839 P1).
//!
//! `GET {base_origin}/api/monitor/usage/quota/limit` (Bearer key) returns
//! either a JSON envelope carrying `data.limits[]` or a plain-text
//! business error (a non-Coding-Plan key gets a bare `Unauthorized` body).
//! Limit rows are tagged `TOKENS_LIMIT` (legacy) or `CREDIT_LIMIT` (new
//! Zhipu billing for GLM Max/Lite); `TIME_LIMIT` rows describe MCP/Add-on
//! service quotas and are ignored. The 5-hour and weekly windows are
//! identified by their `unit`/`number` pair alone (5h: `unit` 3, `number` 5;
//! weekly: `unit` 6, `number` 1 — public OmniRoute `glm.ts` evidence for
//! both limit types), independent of `nextResetTime`. Rows without a
//! supported pair are unknown and dropped rather than guessed; duplicate
//! same-kind rows resolve conservatively to the greatest used share with a
//! deterministic tie-break, retaining the whole winning row.

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

/// JSON envelope business error. Zhipu returns `{code, msg, success}`
/// even on HTTP 200. The official monitor API and the third-party SDK
/// docs both treat `code` 0 and 200 as success; anything else is a
/// rejection. The legacy `code == 0`-only check misread every live
/// success envelope as a failure and surfaced the success message
/// ("操作成功") as the user-facing error.
pub(crate) fn glm_envelope_error(body: &Value) -> Option<(Option<String>, String)> {
    if let Some(code) = body.get("code").and_then(value_as_i64) {
        if code == 0 || code == 200 {
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

// The `unit`/`number` pair alone identifies the window (public evidence:
// OmniRoute `glm.ts` maps 3/5 to the 5-hour window and 6/1 to the weekly
// window for both TOKENS_LIMIT and CREDIT_LIMIT). Reset time never changes
// the kind; rows without a supported pair are unknown and the caller drops
// them rather than guessing a 5h default.
fn classify_window(row: &Value) -> Option<GlmWindowKind> {
    let unit = row.get("unit").and_then(value_as_i64)?;
    let number = row.get("number").and_then(value_as_i64)?;
    match (unit, number) {
        (3, 5) => Some(GlmWindowKind::FiveHour),
        (6, 1) => Some(GlmWindowKind::Weekly),
        _ => None,
    }
}

fn parse_window_row(row: &Value) -> Option<(GlmWindowKind, GlmWindowUsage)> {
    let kind = classify_window(row)?;
    let limit = row.get("limit").and_then(value_as_f64).unwrap_or(0.0);
    let current = row.get("currentValue").and_then(value_as_f64);
    let current_value = current.unwrap_or(0.0);
    let remaining = row.get("remaining").and_then(value_as_f64).unwrap_or(0.0);
    let percentage = derive_percentage(
        row.get("percentage").and_then(value_as_f64),
        current,
        row.get("limit").and_then(value_as_f64),
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

fn derive_percentage(
    reported: Option<f64>,
    current: Option<f64>,
    limit: Option<f64>,
) -> Option<f64> {
    if let Some(value) = reported
        && value.is_finite()
    {
        return Some(value.clamp(0.0, 100.0));
    }
    // A missing current value must never manufacture 0% used from a positive
    // limit: derive only from an actually present finite current and a
    // finite positive limit. A non-finite total (inf/-inf/NaN via numeric
    // strings) is not evidence of zero usage or available capacity.
    if let (Some(current_value), Some(limit_value)) = (current, limit)
        && limit_value.is_finite()
        && limit_value > 0.0
        && current_value.is_finite()
    {
        let derived = current_value / limit_value * 100.0;
        if derived.is_finite() {
            return Some(derived.clamp(0.0, 100.0));
        }
    }
    None
}

/// Conservative duplicate resolution: the greatest used share wins and the
/// whole winning row is retained. An unknown (unreported and underivable)
/// percentage ranks below any known one. Ties compare field-by-field without
/// mixing: greatest current value, then lowest positive limit, then earliest
/// reset, then lowest remaining. A missing reset sorts after dated ones; a
/// missing or zero limit never counts as positive.
fn is_tighter_window(candidate: &GlmWindowUsage, current: &GlmWindowUsage) -> bool {
    match (candidate.percentage, current.percentage) {
        (Some(candidate_used), Some(current_used)) => {
            if candidate_used != current_used {
                return candidate_used > current_used;
            }
        }
        (Some(_), None) => return true,
        (None, Some(_)) => return false,
        (None, None) => {}
    }
    if candidate.current_value != current.current_value {
        return candidate
            .current_value
            .total_cmp(&current.current_value)
            .is_gt();
    }
    match (candidate.limit > 0.0, current.limit > 0.0) {
        (true, false) => return true,
        (false, true) => return false,
        _ => {}
    }
    if candidate.limit != current.limit {
        return candidate.limit < current.limit;
    }
    match (&candidate.next_reset_at, &current.next_reset_at) {
        (Some(candidate_reset), Some(current_reset)) => {
            if candidate_reset != current_reset {
                return candidate_reset < current_reset;
            }
        }
        (Some(_), None) => return true,
        (None, Some(_)) => return false,
        (None, None) => {}
    }
    candidate.remaining < current.remaining
}

/// Top-level parser. Walks `data.limits[]`, drops `TIME_LIMIT` rows and
/// unresolvable rows, then surfaces `None` for empty / malformed /
/// unknown-only bodies so the fetcher can report the honest failure.
/// Duplicate same-kind rows resolve conservatively (see
/// [`is_tighter_window`]) independent of row order. `now` is kept
/// API-compatible for callers; classification no longer consults the clock.
pub(crate) fn parse_glm_quota(body: &Value, _now: DateTime<Utc>) -> Option<GlmQuotaUsage> {
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
        if let Some((kind, window)) = parse_window_row(&Value::Object(object.clone())) {
            let slot = match kind {
                GlmWindowKind::FiveHour => &mut usage.five_hour,
                GlmWindowKind::Weekly => &mut usage.weekly,
            };
            let replace = match slot {
                None => true,
                Some(current) => is_tighter_window(&window, current),
            };
            if replace {
                *slot = Some(window);
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
        // billing) must round-trip into the typed windows. Identity comes
        // from the evidenced unit/number pairs (3/5 and 6/1); the reset
        // deltas below agree with the pairs.
        let body = json!({
            "code": 0,
            "msg": "success",
            "data": {"limits": [
                {"type": "TOKENS_LIMIT", "unit": 3, "number": 5,
                 "currentValue": 1000.0, "limit": 50000.0, "remaining": 49000.0,
                 "percentage": 2.0, "nextResetTime": five_hour_reset()},
                {"type": "CREDIT_LIMIT", "unit": 6, "number": 1,
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
                {"type": "TOKENS_LIMIT", "unit": 3, "number": 5,
                 "limit": 50000, "currentValue": 100, "remaining": 49900, "nextResetTime": five_hour_reset()},
                {"type": "TIME_LIMIT", "limit": 0, "currentValue": 0, "remaining": 0, "nextResetTime": five_hour_reset() + 86_400}
            ]}
        });
        let usage = parse_glm_quota(&body, Utc::now()).expect("quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_none());
        let legacy = json!({
            "code": 0,
            "data": {"limits": [{
                "unit": 3, "number": 5,
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
                "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
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
                    "type": "CREDIT_LIMIT", "unit": 6, "number": 1,
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
                    "type": "CREDIT_LIMIT", "unit": 6, "number": 1,
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

    fn pinned_now() -> DateTime<Utc> {
        // Fixed instant so classification tests never depend on the wall
        // clock (classification itself no longer consults reset time).
        DateTime::parse_from_rfc3339("2026-10-11T00:00:00Z")
            .expect("pinned now")
            .with_timezone(&Utc)
    }

    fn row(
        kind: &str,
        unit: serde_json::Value,
        number: serde_json::Value,
        current: Option<f64>,
        limit: Option<f64>,
        percentage: Option<f64>,
        reset: Option<i64>,
    ) -> serde_json::Value {
        let mut row = serde_json::Map::new();
        row.insert("type".into(), kind.into());
        row.insert("unit".into(), unit);
        row.insert("number".into(), number);
        if let Some(value) = current {
            row.insert("currentValue".into(), value.into());
        }
        if let Some(value) = limit {
            row.insert("limit".into(), value.into());
        }
        if let Some(value) = percentage {
            row.insert("percentage".into(), value.into());
        }
        if let Some(value) = reset {
            row.insert("nextResetTime".into(), value.into());
        }
        serde_json::Value::Object(row)
    }

    fn quota(rows: Vec<serde_json::Value>) -> serde_json::Value {
        json!({"code": 0, "data": {"limits": rows}})
    }

    #[test]
    fn supported_pairs_identify_windows_for_both_limit_types() {
        // The evidenced pairs hold for TOKENS_LIMIT and CREDIT_LIMIT alike.
        let now = pinned_now();
        let body = quota(vec![
            row(
                "CREDIT_LIMIT",
                3.into(),
                5.into(),
                Some(10.0),
                Some(100.0),
                Some(10.0),
                Some(now.timestamp() + 5 * 3600),
            ),
            row(
                "TOKENS_LIMIT",
                6.into(),
                1.into(),
                Some(20.0),
                Some(200.0),
                Some(10.0),
                Some(now.timestamp() + 7 * 24 * 3600),
            ),
        ]);
        let usage = parse_glm_quota(&body, now).expect("quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_some());
    }

    #[test]
    fn string_units_classify_like_numbers() {
        // Numeric strings are valid pair members via value_as_i64.
        let now = pinned_now();
        let body = quota(vec![
            row(
                "TOKENS_LIMIT",
                "3".into(),
                "5".into(),
                Some(0.0),
                Some(100.0),
                Some(0.0),
                None,
            ),
            row(
                "TOKENS_LIMIT",
                "6".into(),
                "1".into(),
                Some(0.0),
                Some(100.0),
                Some(0.0),
                None,
            ),
        ]);
        let usage = parse_glm_quota(&body, now).expect("quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_some());
    }

    #[test]
    fn inconsistent_deadline_defers_to_pair() {
        // A weekly pair resetting within the hour stays weekly; a 5h pair
        // resetting in a week stays 5h. Reset time never changes the kind.
        let now = pinned_now();
        let body = quota(vec![
            row(
                "TOKENS_LIMIT",
                6.into(),
                1.into(),
                Some(90.0),
                Some(100.0),
                Some(90.0),
                Some(now.timestamp() + 3600),
            ),
            row(
                "TOKENS_LIMIT",
                3.into(),
                5.into(),
                Some(10.0),
                Some(100.0),
                Some(10.0),
                Some(now.timestamp() + 7 * 24 * 3600),
            ),
        ]);
        let usage = parse_glm_quota(&body, now).expect("quota");
        let weekly = usage.weekly.expect("weekly despite near reset");
        assert_eq!(weekly.percentage, Some(90.0));
        let five = usage.five_hour.expect("5h despite far reset");
        assert_eq!(five.percentage, Some(10.0));
    }

    #[test]
    fn pair_identity_survives_past_far_and_missing_resets() {
        let now = pinned_now();
        for reset in [
            Some(now.timestamp() - 2 * 3600),
            Some(now.timestamp() + 30 * 24 * 3600),
            None,
        ] {
            let body = quota(vec![row(
                "TOKENS_LIMIT",
                3.into(),
                5.into(),
                Some(1.0),
                Some(100.0),
                Some(1.0),
                reset,
            )]);
            let usage = parse_glm_quota(&body, now).expect("quota");
            assert!(usage.five_hour.is_some(), "reset {reset:?} keeps 5h");
            assert!(usage.weekly.is_none());
            let body = quota(vec![row(
                "TOKENS_LIMIT",
                6.into(),
                1.into(),
                Some(1.0),
                Some(100.0),
                Some(1.0),
                reset,
            )]);
            let usage = parse_glm_quota(&body, now).expect("quota");
            assert!(usage.weekly.is_some(), "reset {reset:?} keeps weekly");
            assert!(usage.five_hour.is_none());
        }
    }

    #[test]
    fn unsupported_and_missing_pairs_are_dropped_honestly() {
        let now = pinned_now();
        // Each fabricated/guessed mapping alone must yield no quota.
        for (unit, number) in [
            (json!(5), json!(1)),
            (json!(3), json!(7)),
            (json!(7), json!(3)),
            (json!(0), json!(0)),
        ] {
            let body = quota(vec![row(
                "TOKENS_LIMIT",
                unit,
                number,
                Some(50.0),
                Some(100.0),
                Some(50.0),
                Some(now.timestamp() + 3600),
            )]);
            assert!(
                parse_glm_quota(&body, now).is_none(),
                "unsupported pair must not establish a window"
            );
        }
        // A row without any pair cannot establish a period either.
        let body = quota(vec![row(
            "TOKENS_LIMIT",
            serde_json::Value::Null,
            serde_json::Value::Null,
            Some(50.0),
            Some(100.0),
            Some(50.0),
            Some(now.timestamp() + 3600),
        )]);
        // Null unit/number fail pair extraction, so the row drops.
        assert!(parse_glm_quota(&body, now).is_none());
        // An unknown-only body is an honest None, never fake usable quota.
        let body = quota(vec![
            row(
                "TOKENS_LIMIT",
                5.into(),
                1.into(),
                Some(50.0),
                Some(100.0),
                Some(50.0),
                Some(now.timestamp() + 3600),
            ),
            row(
                "CREDIT_LIMIT",
                3.into(),
                7.into(),
                Some(10.0),
                Some(100.0),
                Some(10.0),
                Some(now.timestamp() + 7 * 24 * 3600),
            ),
        ]);
        assert!(parse_glm_quota(&body, now).is_none());
    }

    #[test]
    fn partial_unknown_keeps_the_known_window() {
        let now = pinned_now();
        let body = quota(vec![
            row(
                "TOKENS_LIMIT",
                3.into(),
                5.into(),
                Some(10.0),
                Some(100.0),
                Some(10.0),
                Some(now.timestamp() + 5 * 3600),
            ),
            row(
                "TOKENS_LIMIT",
                5.into(),
                1.into(),
                Some(90.0),
                Some(100.0),
                Some(90.0),
                Some(now.timestamp() + 3600),
            ),
        ]);
        let usage = parse_glm_quota(&body, now).expect("quota");
        assert!(usage.five_hour.is_some());
        assert!(usage.weekly.is_none());
    }

    #[test]
    fn missing_current_never_manufactures_percentage() {
        // A positive limit with no current value must not invent 0% used.
        let now = pinned_now();
        let body = quota(vec![row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            None,
            Some(100.0),
            None,
            Some(now.timestamp() + 5 * 3600),
        )]);
        let five = parse_glm_quota(&body, now)
            .expect("quota")
            .five_hour
            .expect("row is kept");
        assert!(five.percentage.is_none());
        // A valid reported percentage still applies without current/limit.
        let body = quota(vec![row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            None,
            None,
            Some(30.0),
            Some(now.timestamp() + 5 * 3600),
        )]);
        let five = parse_glm_quota(&body, now)
            .expect("quota")
            .five_hour
            .expect("row is kept");
        assert_eq!(five.percentage, Some(30.0));
    }

    #[test]
    fn duplicate_rows_resolve_conservatively_independent_of_order() {
        // Greatest used share wins and the whole winning row is retained,
        // regardless of row order.
        let now = pinned_now();
        let light = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            Some(20.0),
            Some(100.0),
            Some(20.0),
            Some(now.timestamp() + 5 * 3600),
        );
        let heavy = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            Some(100.0),
            Some(200.0),
            Some(50.0),
            Some(now.timestamp() + 5 * 3600),
        );
        for rows in [
            vec![light.clone(), heavy.clone()],
            vec![heavy.clone(), light],
        ] {
            let five = parse_glm_quota(&quota(rows), now)
                .expect("quota")
                .five_hour
                .expect("5h");
            assert_eq!(five.percentage, Some(50.0));
            assert_eq!(five.limit, 200.0);
            assert_eq!(five.current_value, 100.0);
        }
    }

    #[test]
    fn duplicate_ties_prefer_greater_current_then_lower_limit() {
        // Equal reported shares tie-break on greatest current value, then
        // lowest positive limit; the whole winning row is retained.
        let now = pinned_now();
        let small = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            Some(40.0),
            Some(200.0),
            Some(50.0),
            Some(now.timestamp() + 5 * 3600),
        );
        let big = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            Some(50.0),
            Some(100.0),
            Some(50.0),
            Some(now.timestamp() + 5 * 3600),
        );
        for rows in [vec![small.clone(), big.clone()], vec![big.clone(), small]] {
            let five = parse_glm_quota(&quota(rows), now)
                .expect("quota")
                .five_hour
                .expect("5h");
            assert_eq!(five.current_value, 50.0);
            assert_eq!(five.limit, 100.0);
        }
        // Unknown percentages rank below any known one: the unknown row
        // carries no current value, so nothing can be derived for it.
        let unknown = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            None,
            Some(100.0),
            None,
            Some(now.timestamp() + 5 * 3600),
        );
        let known = row(
            "TOKENS_LIMIT",
            3.into(),
            5.into(),
            Some(1.0),
            Some(100.0),
            Some(1.0),
            Some(now.timestamp() + 5 * 3600),
        );
        for rows in [vec![unknown.clone(), known.clone()], vec![known, unknown]] {
            let five = parse_glm_quota(&quota(rows), now)
                .expect("quota")
                .five_hour
                .expect("5h");
            assert_eq!(five.percentage, Some(1.0));
        }
    }

    #[test]
    fn next_reset_supports_epoch_seconds_and_milliseconds() {
        let iso = json!("2026-08-22T14:00:00.000Z");
        assert!(glm_next_reset(&json!(five_hour_reset())).is_some());
        assert!(glm_next_reset(&json!(five_hour_reset() * 1000)).is_some());
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
