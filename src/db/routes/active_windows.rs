//! Issue #378 Phase I: per-target effective time windows.
//!
//! `active_windows` is a JSON array of `{start,end,days?}` stored as
//! `TEXT NULL` (`NULL`/empty means unrestricted). `end < start` is overnight
//! (e.g. `22:00-06:00`, weekday decided by start day); overlaps are allowed
//! (any hit means active). `days` is 1=Mon..7=Sun, omitted/empty means every
//! day. All times are interpreted in the worker-local timezone; multi-machine
//! deployments must share one timezone.
//!
//! `start` is `00:00-23:59`; `end` additionally accepts `24:00` (=1440,
//! exclusive midnight), so `00:00-24:00` is a full day and `18:00-24:00`
//! covers the `23:59` minute. Windows stay start-inclusive/end-exclusive and
//! seconds are dropped.

use crate::db::types::ActiveWindow;

/// Parse `HH:MM` into minutes since midnight. `allow_end_of_day` accepts the
/// single extra value `24:00` (1440) for `end`, meaning exclusive midnight;
/// `start` keeps it rejected.
fn parse_hhmm(value: &str, allow_end_of_day: bool) -> Option<u16> {
    let (hour_str, min_str) = value.split_once(':')?;
    if hour_str.len() != 2 || min_str.len() != 2 {
        return None;
    }
    if !hour_str.bytes().all(|b| b.is_ascii_digit()) || !min_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let hour: u16 = hour_str.parse().ok()?;
    let min: u16 = min_str.parse().ok()?;
    if min > 59 {
        return None;
    }
    if hour > 23 {
        // Only `end` may use `24:00`; anything past it (`24:01`..) stays invalid.
        if allow_end_of_day && hour == 24 && min == 0 {
            return Some(1440);
        }
        return None;
    }
    Some(hour * 60 + min)
}

fn validate_window(start: &str, end: &str) -> Result<(u16, u16), &'static str> {
    let start_min =
        parse_hhmm(start, false).ok_or("active_windows start must be HH:MM (00:00-23:59)")?;
    let end_min = parse_hhmm(end, true)
        .ok_or("active_windows end must be HH:MM (00:00-24:00; 24:00 means midnight)")?;
    if start_min == end_min {
        return Err("active_windows start must not equal end (use 00:00-24:00 for all day)");
    }
    Ok((start_min, end_min))
}

fn validate_days(days: &Option<Vec<u8>>) -> Result<(), &'static str> {
    if let Some(values) = days {
        for day in values {
            if *day < 1 || *day > 7 {
                return Err("active_windows days must be 1..7");
            }
        }
    }
    Ok(())
}

/// Validate request windows and return the sorted-normalized clone.
/// Empty means unrestricted. Overlaps are allowed; sort is by start then end.
pub fn normalize_request_windows(
    windows: &[ActiveWindow],
) -> Result<Vec<ActiveWindow>, &'static str> {
    for window in windows {
        validate_window(window.start.trim(), window.end.trim())?;
        validate_days(&window.days)?;
    }
    let mut normalized = windows
        .iter()
        .map(|window| ActiveWindow {
            start: window.start.trim().to_string(),
            end: window.end.trim().to_string(),
            days: window.days.clone(),
        })
        .collect::<Vec<_>>();
    normalized.sort_by_key(|window| {
        (
            parse_hhmm(&window.start, false).unwrap_or(u16::MAX),
            parse_hhmm(&window.end, true).unwrap_or(u16::MAX),
        )
    });
    Ok(normalized)
}

/// Serialize normalized windows for storage: empty means all-day (`None`
/// maps to SQL `NULL`).
pub fn storage_value(windows: &[ActiveWindow]) -> Option<String> {
    if windows.is_empty() {
        return None;
    }
    serde_json::to_string(windows).ok()
}

/// Parse a stored `active_windows` value. `None`/empty/whitespace means
/// all-day (`Ok(vec![])`). Invalid stored JSON is an error so routing can
/// fail closed instead of silently treating it as all-day.
pub fn parse_stored_windows(raw: Option<&str>) -> Result<Vec<ActiveWindow>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    let windows: Vec<ActiveWindow> = serde_json::from_str(raw)
        .map_err(|_| "active_windows is not a valid JSON array".to_string())?;
    normalize_request_windows(&windows).map_err(str::to_string)
}

/// `end_min` may be 1440 for `24:00` (exclusive midnight); `now_min` is
/// always 0..=1439. `end < start` is overnight, `end` is exclusive.
fn window_covers(start_min: u16, end_min: u16, now_min: u16) -> bool {
    if end_min > start_min {
        now_min >= start_min && now_min < end_min
    } else {
        now_min >= start_min || now_min < end_min
    }
}

/// Whether `now_min` (minutes since local midnight) is inside any window.
/// Empty means unrestricted. Assumes already-validated windows; unparseable
/// entries never match (fail-closed).
pub fn is_active_at(windows: &[ActiveWindow], now_min: u16) -> bool {
    is_active_now(windows, weekday_now(), now_min)
}

/// Weekday now, 1=Mon..7=Sun, worker-local.
pub fn weekday_now() -> u8 {
    use chrono::Datelike;
    chrono::Local::now().weekday().number_from_monday() as u8
}

fn window_hits_day(days: &Option<Vec<u8>>, today: u8) -> bool {
    match days {
        None => true,
        Some(values) if values.is_empty() => true,
        Some(values) => values.contains(&today),
    }
}

/// Whether windows are active at weekday+minutes. Empty means unrestricted.
/// Overnight windows are decided by start day (Mon 22:00-06:00 does not cover Tue 01:00).
pub fn is_active_now(windows: &[ActiveWindow], today: u8, now_min: u16) -> bool {
    if windows.is_empty() {
        return true;
    }
    windows.iter().any(|window| {
        if !window_hits_day(&window.days, today) {
            return false;
        }
        match (
            parse_hhmm(window.start.trim(), false),
            parse_hhmm(window.end.trim(), true),
        ) {
            (Some(start_min), Some(end_min)) if start_min != end_min => {
                window_covers(start_min, end_min, now_min)
            }
            _ => false,
        }
    })
}

/// Whether a stored raw value is active at `now_min`. `None`/empty means
/// unrestricted; corrupt stored JSON means inactive (fail-closed).
pub fn stored_is_active_at(raw: Option<&str>, now_min: u16) -> bool {
    stored_is_active_now(raw, weekday_now(), now_min)
}

/// Whether a stored raw value is active at weekday+minutes.
pub fn stored_is_active_now(raw: Option<&str>, today: u8, now_min: u16) -> bool {
    match parse_stored_windows(raw) {
        Ok(windows) => is_active_now(&windows, today, now_min),
        Err(_) => false,
    }
}

/// Worker-local minutes since midnight.
pub fn worker_local_minutes_now() -> u16 {
    let now = chrono::Local::now().time();
    #[allow(clippy::cast_possible_truncation)]
    let hour = now.hour() as u16;
    #[allow(clippy::cast_possible_truncation)]
    let min = now.minute() as u16;
    hour * 60 + min
}

/// Worker-local `HH:MM` for fail-closed messages.
pub fn worker_local_hhmm_now() -> String {
    format_minutes(worker_local_minutes_now())
}

pub fn format_minutes(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

/// Compact windows summary for fail-closed messages: `unrestricted` or
/// `06:30-14:00, 18:00-20:00`.
pub fn summarize_windows(windows: &[ActiveWindow]) -> String {
    if windows.is_empty() {
        return "unrestricted".to_string();
    }
    windows
        .iter()
        .map(|window| format!("{}-{}", window.start.trim(), window.end.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Compact summary for a stored raw value (corrupt reads as `invalid`).
pub fn summarize_stored(raw: Option<&str>) -> String {
    match parse_stored_windows(raw) {
        Ok(windows) => summarize_windows(&windows),
        Err(_) => "invalid".to_string(),
    }
}

/// Issue #392 Phase K: effective windows for inheritance.
/// Target-nonempty wins; otherwise the endpoint default applies;
/// both empty means all-day (`None`). Nonempty is trimmed-nonempty
/// stored text (NULL/empty/whitespace counts as empty).
pub fn effective_windows_raw<'a>(
    target_raw: Option<&'a str>,
    endpoint_raw: Option<&'a str>,
) -> Option<&'a str> {
    if target_raw.map(str::trim).is_some_and(|v| !v.is_empty()) {
        target_raw
    } else if endpoint_raw.map(str::trim).is_some_and(|v| !v.is_empty()) {
        endpoint_raw
    } else {
        None
    }
}

/// Whether the effective windows are active at `now_min`.
/// Target-nonempty else endpoint else unrestricted; corrupt effective JSON
/// means inactive (fail-closed).
pub fn effective_stored_is_active_at(
    target_raw: Option<&str>,
    endpoint_raw: Option<&str>,
    now_min: u16,
) -> bool {
    effective_stored_is_active_now(target_raw, endpoint_raw, weekday_now(), now_min)
}

/// Whether the effective windows are active at weekday+minutes.
pub fn effective_stored_is_active_now(
    target_raw: Option<&str>,
    endpoint_raw: Option<&str>,
    today: u8,
    now_min: u16,
) -> bool {
    stored_is_active_now(
        effective_windows_raw(target_raw, endpoint_raw),
        today,
        now_min,
    )
}

/// Compact summary for effective windows (corrupt reads as `invalid`).
pub fn summarize_effective_stored(target_raw: Option<&str>, endpoint_raw: Option<&str>) -> String {
    summarize_stored(effective_windows_raw(target_raw, endpoint_raw))
}

/// Whether a candidate target participates at `now_min`. `enabled=false`
/// is always out, even when its windows would otherwise match.
/// Effective windows resolve as target-nonempty else endpoint else all-day.
pub fn candidate_target_is_active(
    target: &crate::db::types::ModelRouteCandidateTarget,
    now_min: u16,
) -> bool {
    if !target.enabled {
        return false;
    }
    effective_stored_is_active_at(
        target.active_windows.as_deref(),
        target.endpoint_active_windows.as_deref(),
        now_min,
    )
}

/// Fail-closed message when schedule filtering leaves no target. Carries
/// the route pattern, worker-local time, and per-target windows summaries.
/// Summaries show effective windows (target-nonempty else endpoint).
pub fn schedule_unavailable_message(
    candidate: &crate::db::types::ModelRouteCandidate,
    now_min: u16,
) -> String {
    let now = format_minutes(now_min);
    let summaries = candidate
        .targets
        .iter()
        .map(|target| {
            format!(
                "{}({}): {}",
                target.endpoint_name,
                if target.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                summarize_effective_stored(
                    target.active_windows.as_deref(),
                    target.endpoint_active_windows.as_deref()
                ),
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "no route target is active for route '{}' at {} (worker-local time; windows: {})",
        candidate.model_pattern, now, summaries
    )
}

use chrono::Timelike;

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start: &str, end: &str) -> ActiveWindow {
        ActiveWindow {
            start: start.to_string(),
            end: end.to_string(),
            days: None,
        }
    }

    fn window_days(start: &str, end: &str, days: &[u8]) -> ActiveWindow {
        ActiveWindow {
            start: start.to_string(),
            end: end.to_string(),
            days: Some(days.to_vec()),
        }
    }

    #[test]
    fn empty_means_all_day() {
        assert!(is_active_at(&[], 0));
        assert!(is_active_at(&[], 720));
        assert!(is_active_at(&[], 1439));
        assert!(stored_is_active_at(None, 720));
        assert!(stored_is_active_at(Some(""), 720));
        assert!(stored_is_active_at(Some("   "), 720));
        assert_eq!(summarize_windows(&[]), "unrestricted");
    }

    #[test]
    fn weekday_filtering() {
        let windows = vec![window_days("00:00", "08:00", &[1, 2, 3, 4, 5])];
        assert!(is_active_now(&windows, 3, 60));
        assert!(!is_active_now(&windows, 7, 60));
        assert!(!is_active_now(&windows, 6, 60));
        // None/empty means every day
        assert!(is_active_now(&[window("00:00", "08:00")], 7, 60));
        assert!(is_active_now(&[window_days("00:00", "08:00", &[])], 7, 60));
    }

    #[test]
    fn invalid_days_rejected() {
        assert!(validate_days(&Some(vec![0])).is_err());
        assert!(validate_days(&Some(vec![9])).is_err());
        assert!(validate_days(&Some(vec![1, 7])).is_ok());
    }

    #[test]
    fn overnight_uses_start_day() {
        let windows = vec![window_days("22:00", "06:00", &[1])];
        assert!(is_active_now(&windows, 1, 23 * 60));
        assert!(!is_active_now(&windows, 2, 60));
    }

    #[test]
    fn boundary_minutes_are_start_inclusive_end_exclusive() {
        let windows = vec![window("06:30", "14:00")];
        assert!(!is_active_at(&windows, 6 * 60 + 29));
        assert!(is_active_at(&windows, 6 * 60 + 30));
        assert!(is_active_at(&windows, 13 * 60 + 59));
        assert!(!is_active_at(&windows, 14 * 60));
    }

    #[test]
    fn overnight_window_wraps_midnight() {
        let windows = vec![window("22:00", "06:00")];
        assert!(is_active_at(&windows, 22 * 60));
        assert!(is_active_at(&windows, 23 * 60 + 59));
        assert!(is_active_at(&windows, 0));
        assert!(is_active_at(&windows, 5 * 60 + 59));
        assert!(!is_active_at(&windows, 6 * 60));
        assert!(!is_active_at(&windows, 12 * 60));
    }

    #[test]
    fn end_2400_is_midnight_and_only_valid_as_end() {
        // Issue #457: `end` may use `24:00` (=1440); `start` may not.
        assert!(normalize_request_windows(&[window("00:00", "24:00")]).is_ok());
        assert!(normalize_request_windows(&[window("18:00", "24:00")]).is_ok());
        assert!(normalize_request_windows(&[window("23:59", "24:00")]).is_ok());
        assert!(normalize_request_windows(&[window("24:00", "23:59")]).is_err());
        assert!(normalize_request_windows(&[window("24:01", "23:59")]).is_err());
        assert!(normalize_request_windows(&[window("00:00", "24:01")]).is_err());
        // `00:00-00:00` stays rejected; full day is `00:00-24:00`.
        assert!(normalize_request_windows(&[window("00:00", "00:00")]).is_err());
    }

    #[test]
    fn full_day_window_covers_every_minute() {
        let windows = vec![window("00:00", "24:00")];
        assert!(is_active_at(&windows, 0));
        assert!(is_active_at(&windows, 720));
        assert!(is_active_at(&windows, 1439));
    }

    #[test]
    fn evening_window_ending_2400_covers_2359() {
        let windows = vec![window("18:00", "24:00")];
        assert!(is_active_at(&windows, 18 * 60));
        assert!(is_active_at(&windows, 1080));
        assert!(is_active_at(&windows, 1439));
        assert!(!is_active_at(&windows, 17 * 60 + 59));
        assert!(!is_active_at(&windows, 0));
    }

    #[test]
    fn end_0000_keeps_overnight_compatibility() {
        // Issue #457: `18:00-00:00` keeps its prior overnight semantics
        // (evening only, `24:00` is the way to express midnight).
        let windows = vec![window("18:00", "00:00")];
        assert!(is_active_at(&windows, 18 * 60));
        assert!(is_active_at(&windows, 1439));
        assert!(!is_active_at(&windows, 0));
    }

    #[test]
    fn overlap_means_any_hit_is_active() {
        let windows = vec![window("06:00", "08:00"), window("07:30", "09:00")];
        assert!(is_active_at(&windows, 6 * 60));
        assert!(is_active_at(&windows, 7 * 60 + 45));
        assert!(is_active_at(&windows, 8 * 60 + 30));
        assert!(!is_active_at(&windows, 9 * 60));
    }

    #[test]
    fn fail_closed_on_empty_result_and_corrupt_store() {
        let windows = vec![window("06:00", "07:00")];
        assert!(!is_active_at(&windows, 12 * 60));
        assert!(!stored_is_active_at(Some("not-json"), 12 * 60));
        assert_eq!(summarize_stored(Some("not-json")), "invalid");
    }

    #[test]
    fn invalid_formats_are_rejected() {
        assert!(normalize_request_windows(&[window("6:30", "14:00")]).is_err());
        assert!(normalize_request_windows(&[window("06:60", "14:00")]).is_err());
        assert!(normalize_request_windows(&[window("24:00", "14:00")]).is_err());
        assert!(normalize_request_windows(&[window("06:30", "06:30")]).is_err());
        assert!(normalize_request_windows(&[window("", "14:00")]).is_err());
    }

    #[test]
    fn sorted_normalize_orders_by_start_then_end() {
        let out = normalize_request_windows(&[window("18:00", "20:00"), window("06:30", "14:00")])
            .expect("valid");
        assert_eq!(out[0].start, "06:30");
        assert_eq!(out[1].start, "18:00");
    }

    #[test]
    fn effective_windows_resolve_target_nonempty_else_endpoint_else_all_day() {
        // Issue #392 Phase K: inheritance tristate — target wins when
        // nonempty, otherwise endpoint, otherwise all-day.
        let target_day = r#"[{"start":"06:00","end":"07:00"}]"#;
        let endpoint_night = r#"[{"start":"22:00","end":"23:00"}]"#;
        assert_eq!(
            effective_windows_raw(Some(target_day), Some(endpoint_night)),
            Some(target_day)
        );
        assert_eq!(
            effective_windows_raw(None, Some(endpoint_night)),
            Some(endpoint_night)
        );
        assert_eq!(
            effective_windows_raw(Some(""), Some(endpoint_night)),
            Some(endpoint_night)
        );
        assert_eq!(effective_windows_raw(Some("   "), None), None);
        assert_eq!(effective_windows_raw(None, None), None);
        assert_eq!(effective_windows_raw(Some(""), Some("  ")), None);
        // Effective activity follows the resolved value.
        assert!(effective_stored_is_active_at(
            Some(target_day),
            Some(endpoint_night),
            6 * 60 + 30
        ));
        assert!(!effective_stored_is_active_at(
            Some(target_day),
            Some(endpoint_night),
            22 * 60 + 30
        ));
        assert!(effective_stored_is_active_at(
            None,
            Some(endpoint_night),
            22 * 60 + 30
        ));
        assert!(!effective_stored_is_active_at(
            None,
            Some(endpoint_night),
            12 * 60
        ));
        assert!(effective_stored_is_active_at(None, None, 12 * 60));
        assert_eq!(
            summarize_effective_stored(Some(target_day), Some(endpoint_night)),
            "06:00-07:00"
        );
        assert_eq!(
            summarize_effective_stored(None, Some(endpoint_night)),
            "22:00-23:00"
        );
        assert_eq!(summarize_effective_stored(None, None), "unrestricted");
    }

    #[test]
    fn candidate_inherits_endpoint_windows_when_target_empty() {
        use crate::db::types::ModelRouteCandidateTarget;
        fn target(
            target_raw: Option<&str>,
            endpoint_raw: Option<&str>,
        ) -> ModelRouteCandidateTarget {
            ModelRouteCandidateTarget {
                target_id: uuid::Uuid::new_v4(),
                endpoint_id: uuid::Uuid::new_v4(),
                endpoint_name: "e".to_string(),
                base_url: "https://e.example".to_string(),
                api_key: "k".to_string(),
                api_keys: Vec::new(),
                key_lb_enabled: false,
                native_api: crate::config::NativeApi::Chat,
                target_native_api: crate::config::NativeApi::Chat,
                position: 0,
                enabled: true,
                upstream_model: None,
                provider: crate::db::EndpointProvider::Generic,
                service_tier: crate::db::MinimaxServiceTier::Standard,
                proxy_url: None,
                proxy_url_override: None,
                active_windows: target_raw.map(str::to_string),
                endpoint_active_windows: endpoint_raw.map(str::to_string),
                dev_system_normalize: false,
                thinking_effort_override: None,
            }
        }
        let day = r#"[{"start":"06:00","end":"07:00"}]"#;
        let night = r#"[{"start":"22:00","end":"23:00"}]"#;
        // Target set wins over endpoint.
        assert!(candidate_target_is_active(
            &target(Some(day), Some(night)),
            6 * 60 + 30
        ));
        assert!(!candidate_target_is_active(
            &target(Some(day), Some(night)),
            22 * 60 + 30
        ));
        // Empty target inherits endpoint.
        assert!(candidate_target_is_active(
            &target(None, Some(night)),
            22 * 60 + 30
        ));
        assert!(!candidate_target_is_active(
            &target(None, Some(night)),
            12 * 60
        ));
        assert!(candidate_target_is_active(
            &target(Some(""), Some(night)),
            22 * 60 + 30
        ));
        // Both empty means all-day.
        assert!(candidate_target_is_active(&target(None, None), 12 * 60));
        assert!(candidate_target_is_active(
            &target(Some("  "), Some(" ")),
            0
        ));
    }

    #[test]
    fn endpoint_windows_reuse_target_hhmm_validation() {
        // Issue #392 Phase K: endpoint windows reuse the same HH:MM helper —
        // invalid endpoint input is rejected the same way as targets.
        assert!(normalize_request_windows(&[window("6:30", "14:00")]).is_err());
        assert!(normalize_request_windows(&[window("06:30", "06:30")]).is_err());
        assert!(normalize_request_windows(&[window("22:00", "06:00")]).is_ok());
        assert!(storage_value(&[]).is_none());
    }

    #[test]
    fn disabled_target_is_always_out_and_fail_closed_message_carries_route_time_windows() {
        use crate::db::types::ModelRouteCandidateTarget;
        let active = ModelRouteCandidateTarget {
            target_id: uuid::Uuid::new_v4(),
            endpoint_id: uuid::Uuid::new_v4(),
            endpoint_name: "day".to_string(),
            base_url: "https://day.example".to_string(),
            api_key: "k".to_string(),
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Chat,
            target_native_api: crate::config::NativeApi::Chat,
            position: 0,
            enabled: false,
            upstream_model: None,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
            proxy_url: None,
            proxy_url_override: None,
            active_windows: None,
            endpoint_active_windows: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
        };
        assert!(!candidate_target_is_active(&active, 720));
        let candidate = crate::db::types::ModelRouteCandidate {
            rule_id: uuid::Uuid::new_v4(),
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: crate::db::types::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            updated_at: chrono::Utc::now(),
            targets: vec![active],
        };
        let message = schedule_unavailable_message(&candidate, 12 * 60);
        assert!(message.contains("gpt-*"), "{message}");
        assert!(message.contains("12:00"), "{message}");
        assert!(message.contains("unrestricted"), "{message}");
    }
}
