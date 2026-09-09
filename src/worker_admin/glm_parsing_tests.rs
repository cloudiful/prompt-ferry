//! P4 (issue #230) regression: the reset-delta classifier owns the
//! 12h boundary on the FiveHour side and the 14d boundary on the
//! Weekly side. The full boundary table is too long to keep inline
//! next to the parser (the P3 reviewer flagged `glm_parsing.rs` at
//! 396 lines; this file lives in a sibling so the parser stays under
//! the 400-line planning cap).

use serde_json::json;

use crate::worker_admin::glm_parsing::{glm_envelope_error, parse_glm_quota};

#[test]
fn weekly_vs_5h_boundary_deltas() {
    // The pin below injects a known `now` so the test is not at the
    // mercy of the wall clock between the two `Utc::now()` calls; the
    // 5-minute buffer past each boundary also outpaces
    // `chrono::TimeDelta::num_minutes` truncation, which rounds down
    // to whole minutes.
    let pinned_now = chrono::Utc::now();
    // 12h0m exactly: FiveHour owns the 12h boundary.
    let boundary_5h = json!({
        "code": 0,
        "data": {"limits": [{
            "type": "TOKENS_LIMIT",
            "limit": 1.0, "currentValue": 0.0,
            "nextResetTime": pinned_now.timestamp() + 12 * 3600
        }]}
    });
    let parsed = parse_glm_quota(&boundary_5h, pinned_now).expect("12h boundary");
    assert!(parsed.five_hour.is_some());
    assert!(parsed.weekly.is_none());
    // 12h + 5m lands inside the Weekly range; the 5h window must be
    // None so the cache does not double-count.
    let just_after_5h = json!({
        "code": 0,
        "data": {"limits": [{
            "type": "TOKENS_LIMIT",
            "limit": 1.0, "currentValue": 0.0,
            "nextResetTime": pinned_now.timestamp() + 12 * 3600 + 5 * 60
        }]}
    });
    let parsed = parse_glm_quota(&just_after_5h, pinned_now).expect("just-after 12h");
    assert!(parsed.five_hour.is_none());
    assert!(parsed.weekly.is_some());
    // 14d exactly: Weekly owns the 14d boundary.
    let boundary_weekly = json!({
        "code": 0,
        "data": {"limits": [{
            "type": "TOKENS_LIMIT",
            "limit": 1.0, "currentValue": 0.0,
            "nextResetTime": pinned_now.timestamp() + 14 * 24 * 3600
        }]}
    });
    let parsed = parse_glm_quota(&boundary_weekly, pinned_now).expect("14d boundary");
    assert!(parsed.five_hour.is_none());
    assert!(parsed.weekly.is_some());
    // 14d - 1h lands inside the Weekly range; pins the upper
    // boundary against off-by-one errors.
    let just_inside_weekly = json!({
        "code": 0,
        "data": {"limits": [{
            "type": "TOKENS_LIMIT",
            "limit": 1.0, "currentValue": 0.0,
            "nextResetTime": pinned_now.timestamp() + (14 * 24 - 1) * 3600
        }]}
    });
    let parsed = parse_glm_quota(&just_inside_weekly, pinned_now).expect("13d23h");
    assert!(parsed.five_hour.is_none());
    assert!(parsed.weekly.is_some());
}

#[test]
fn live_code_200_envelope_is_treated_as_success() {
    // Live monitor response (issue #238): the official API returns
    // `code: 200 + msg: "操作成功" + success: true` for healthy keys
    // alongside a populated `data.limits[]`. The pre-fix envelope
    // check (`code == 0` only) misread every live success as a
    // rejection and surfaced "操作成功" as the user-facing error.
    let body = json!({
        "code": 200,
        "msg": "操作成功",
        "success": true,
        "data": {"limits": [
            {"type": "TOKENS_LIMIT", "unit": 5, "number": 1,
             "currentValue": 1000.0, "limit": 50000.0, "remaining": 49000.0,
             "percentage": 2.0, "nextResetTime": pinned_now().timestamp() + 5 * 3600},
            {"type": "CREDIT_LIMIT", "unit": 3, "number": 7,
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

fn pinned_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}
