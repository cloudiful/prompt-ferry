//! P4 (issue #230) regression: the reset-delta classifier owns the
//! 12h boundary on the FiveHour side and the 14d boundary on the
//! Weekly side. The full boundary table is too long to keep inline
//! next to the parser (the P3 reviewer flagged `glm_parsing.rs` at
//! 396 lines; this file lives in a sibling so the parser stays under
//! the 400-line planning cap).

use serde_json::json;

use crate::worker_admin::glm_parsing::parse_glm_quota;

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
