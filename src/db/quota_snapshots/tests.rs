use super::validate_windows;
use serde_json::{Value, json};

fn known_window(source: &str, remaining: f64) -> Value {
    json!([{
        "source_window": source,
        "window_seconds": 18_000,
        "used_percent": 100.0 - remaining,
        "remaining_percent": remaining,
        "availability": "known"
    }])
}

#[test]
fn normalized_windows_reject_raw_or_inconsistent_payload_fields() {
    validate_windows(&known_window("primary", 0.0)).expect("known zero remains valid");
    validate_windows(&json!([{
        "source_window": "secondary",
        "window_seconds": 604_800,
        "availability": "unknown"
    }]))
    .expect("unknown windows retain known duration");
    assert!(
        validate_windows(
            &json!([{"source_window":"primary","availability":"unknown","account_id":"raw"}])
        )
        .is_err()
    );
    assert!(validate_windows(&json!([{"source_window":"primary","availability":"known","used_percent":120,"remaining_percent":0}])).is_err());
}
