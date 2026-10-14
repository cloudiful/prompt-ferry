//! Issue #839 P1 regressions: window identity comes from the evidenced
//! `unit`/`number` pairs (3/5 five-hour, 6/1 weekly) independent of reset
//! time, and rows without a supported pair are unknown rather than guessed.
//! Reset deltas only travel as `next_reset_at` metadata; the sibling parser
//! module keeps the pair/duplicate/percentage unit tests.

use serde_json::json;

use crate::worker_admin::glm_parsing::{glm_envelope_error, parse_glm_quota};

#[test]
fn reviewer_nonfinite_limit_does_not_manufacture_zero_usage() {
    let body = json!({"data": {"limits": [{
        "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
        "currentValue": 10.0, "limit": "inf",
        "nextResetTime": pinned_now().timestamp() + 3600
    }]}});
    let parsed = parse_glm_quota(&body, pinned_now()).expect("supported identity");
    assert_eq!(
        parsed.five_hour.expect("5h window").percentage,
        None,
        "a nonfinite limit is not evidence of zero usage or available capacity"
    );
}

#[test]
fn nonfinite_limit_boundaries_stay_unknown() {
    // Every non-finite total representation must leave the derived share
    // unknown rather than manufacturing 0% used / 100% remaining.
    // Numeric strings reach value_as_f64 via f64 parsing, so "inf",
    // "-inf", "NaN", and an overflow exponent all arrive non-finite.
    for limit in [
        json!("inf"),
        json!("-inf"),
        json!("NaN"),
        json!("infinity"),
        json!("-infinity"),
        json!("1e400"),
    ] {
        let body = json!({"data": {"limits": [{
            "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
            "currentValue": 10.0, "limit": limit,
            "nextResetTime": pinned_now().timestamp() + 3600
        }]}});
        let parsed = parse_glm_quota(&body, pinned_now()).expect("supported identity");
        assert_eq!(
            parsed.five_hour.expect("5h window").percentage,
            None,
            "non-finite limit {limit} must not derive a percentage"
        );
    }
}

#[test]
fn missing_current_with_nonfinite_limit_stays_unknown() {
    // No current value plus a non-finite total: nothing to derive from.
    let body = json!({"data": {"limits": [{
        "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
        "limit": "inf",
        "nextResetTime": pinned_now().timestamp() + 3600
    }]}});
    let parsed = parse_glm_quota(&body, pinned_now()).expect("supported identity");
    assert_eq!(
        parsed.five_hour.expect("5h window").percentage,
        None,
        "missing current with non-finite limit stays unknown"
    );
}

#[test]
fn valid_reported_percentage_survives_nonfinite_amounts() {
    // A finite reported share is independent of the amounts: it still
    // applies even when the total is non-finite or the current is absent.
    for body in [
        json!({"data": {"limits": [{
            "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
            "currentValue": 10.0, "limit": "inf", "percentage": 30.0,
            "nextResetTime": pinned_now().timestamp() + 3600
        }]}}),
        json!({"data": {"limits": [{
            "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
            "limit": "inf", "percentage": 30.0,
            "nextResetTime": pinned_now().timestamp() + 3600
        }]}}),
    ] {
        let parsed = parse_glm_quota(&body, pinned_now()).expect("supported identity");
        assert_eq!(
            parsed.five_hour.expect("5h window").percentage,
            Some(30.0),
            "finite reported percentage is preserved"
        );
    }
}

#[test]
fn legitimate_zero_current_with_positive_total_derives_zero() {
    // Finite 0 current over a finite positive total is a real 0% used.
    let body = json!({"data": {"limits": [{
        "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
        "currentValue": 0.0, "limit": 100.0,
        "nextResetTime": pinned_now().timestamp() + 3600
    }]}});
    let parsed = parse_glm_quota(&body, pinned_now()).expect("supported identity");
    assert_eq!(
        parsed.five_hour.expect("5h window").percentage,
        Some(0.0),
        "legitimate finite zero usage derives 0%"
    );
}

fn pinned_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-10-11T00:00:00Z")
        .expect("pinned now")
        .with_timezone(&chrono::Utc)
}

fn row(
    kind: &str,
    unit: serde_json::Value,
    number: serde_json::Value,
    reset: Option<i64>,
) -> serde_json::Value {
    let mut row = serde_json::Map::new();
    row.insert("type".into(), kind.into());
    row.insert("unit".into(), unit);
    row.insert("number".into(), number);
    row.insert("limit".into(), 100.0.into());
    row.insert("currentValue".into(), 10.0.into());
    if let Some(reset) = reset {
        row.insert("nextResetTime".into(), reset.into());
    }
    serde_json::Value::Object(row)
}

fn quota(rows: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"code": 0, "data": {"limits": rows}})
}

#[test]
fn weekly_identity_survives_near_and_past_resets() {
    // The headline defect: a weekly window resetting within the hour (or
    // already past) must stay weekly, never collapse to 5h.
    let now = pinned_now();
    for (label, reset) in [
        ("near reset", Some(now.timestamp() + 3600)),
        ("just reset", Some(now.timestamp())),
        ("past reset", Some(now.timestamp() - 2 * 3600)),
        ("far reset", Some(now.timestamp() + 30 * 24 * 3600)),
        ("no reset", None),
    ] {
        let body = quota(vec![row("TOKENS_LIMIT", 6.into(), 1.into(), reset)]);
        let parsed = parse_glm_quota(&body, now).expect(label);
        assert!(parsed.weekly.is_some(), "{label} keeps weekly");
        assert!(parsed.five_hour.is_none(), "{label} never claims 5h");
    }
}

#[test]
fn five_hour_identity_survives_far_and_missing_resets() {
    // A 5h pair resetting days out (stale metadata) stays 5h.
    let now = pinned_now();
    for (label, reset) in [
        ("near reset", Some(now.timestamp() + 5 * 3600)),
        ("far reset", Some(now.timestamp() + 10 * 24 * 3600)),
        ("past reset", Some(now.timestamp() - 3600)),
        ("no reset", None),
    ] {
        let body = quota(vec![row("CREDIT_LIMIT", 3.into(), 5.into(), reset)]);
        let parsed = parse_glm_quota(&body, now).expect(label);
        assert!(parsed.five_hour.is_some(), "{label} keeps 5h");
        assert!(parsed.weekly.is_none(), "{label} never claims weekly");
    }
}

#[test]
fn unidentifiable_rows_yield_no_quota() {
    // Fabricated mappings (5/1, 3/7) and pair-less rows are unknown: an
    // unknown-only body is an honest None, never a defaulted 5h window.
    let now = pinned_now();
    let body = quota(vec![
        row(
            "TOKENS_LIMIT",
            5.into(),
            1.into(),
            Some(now.timestamp() + 3600),
        ),
        row(
            "CREDIT_LIMIT",
            3.into(),
            7.into(),
            Some(now.timestamp() + 7 * 24 * 3600),
        ),
    ]);
    assert!(parse_glm_quota(&body, now).is_none());
    let mut pairless = serde_json::Map::new();
    pairless.insert("type".into(), "TOKENS_LIMIT".into());
    pairless.insert("limit".into(), 100.0.into());
    pairless.insert("currentValue".into(), 10.0.into());
    let body = quota(vec![serde_json::Value::Object(pairless)]);
    assert!(parse_glm_quota(&body, now).is_none());
}

#[test]
fn live_code_200_envelope_is_treated_as_success() {
    // Live monitor response (issue #238): the official API returns
    // `code: 200 + msg: "操作成功" + success: true` for healthy keys
    // alongside a populated `data.limits[]`. The pre-fix envelope
    // check (`code == 0` only) misread every live success as a
    // rejection and surfaced "操作成功" as the user-facing error.
    // Fixtures use the evidenced pairs (3/5 five-hour, 6/1 weekly).
    let body = json!({
        "code": 200,
        "msg": "操作成功",
        "success": true,
        "data": {"limits": [
            {"type": "TOKENS_LIMIT", "unit": 3, "number": 5,
             "currentValue": 1000.0, "limit": 50000.0, "remaining": 49000.0,
             "percentage": 2.0, "nextResetTime": pinned_now().timestamp() + 5 * 3600},
            {"type": "CREDIT_LIMIT", "unit": 6, "number": 1,
             "currentValue": 5.0, "limit": 100.0, "remaining": 95.0,
             "percentage": 5.0, "nextResetTime": pinned_now().timestamp() + 7 * 24 * 3600}
        ]}
    });
    assert!(
        glm_envelope_error(&body).is_none(),
        "live code 200 envelope must be treated as success",
    );
    let usage = parse_glm_quota(&body, pinned_now()).expect("live quota");
    let five = usage.five_hour.expect("5h window from live envelope");
    assert_eq!(five.limit, 50000.0);
    assert_eq!(five.current_value, 1000.0);
    assert_eq!(five.percentage, Some(2.0));
    let weekly = usage.weekly.expect("weekly window from live envelope");
    assert_eq!(weekly.limit, 100.0);
    assert_eq!(weekly.percentage, Some(5.0));
}

#[test]
fn winning_row_carries_its_own_observation_presence() {
    // Duplicate same-kind rows (token + credit): the conservative winner's
    // typed window and its presence-aware canonical observation move
    // together — unit and amounts come from the winner only, in either row
    // order.
    let now = pinned_now();
    let light = json!({
        "type": "TOKENS_LIMIT", "unit": 3, "number": 5,
        "currentValue": 20.0, "limit": 100.0, "remaining": 80.0,
        "percentage": 20.0, "nextResetTime": now.timestamp() + 5 * 3600
    });
    let heavy = json!({
        "type": "CREDIT_LIMIT", "unit": 3, "number": 5,
        "currentValue": 100.0, "limit": 200.0, "remaining": 100.0,
        "percentage": 50.0, "nextResetTime": now.timestamp() + 5 * 3600
    });
    for rows in [vec![light.clone(), heavy.clone()], vec![heavy, light]] {
        let parsed = parse_glm_quota(&quota(rows), now).expect("quota");
        let five = parsed.five_hour.expect("5h window");
        assert_eq!(five.percentage, Some(50.0));
        assert_eq!(five.limit, 200.0);
        assert_eq!(parsed.observations.len(), 1);
        let observation = &parsed.observations[0];
        assert_eq!(observation.source_id, "glm:five_hour");
        // Winner-only presence: credit unit and winner amounts, not mixed
        // with the lighter token row.
        assert_eq!(observation.unit.as_deref(), Some("credits"));
        assert_eq!(observation.current, Some(100.0));
        assert_eq!(observation.limit, Some(200.0));
        assert_eq!(observation.remaining_percent, Some(50.0));
    }
}
