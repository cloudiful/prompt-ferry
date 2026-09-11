//! Urgency-aware window weighting for providers that expose quota windows
//! (issue #281 P1). Each provider keeps its tightest-window bottleneck, but
//! every window's remaining percent is first lifted toward 100 by its reset
//! urgency, so a window that refills soon is penalized less than one that
//! stays depleted for a long time. Windows with no reset signal keep their
//! raw remaining, degrading to the plain min-bottleneck behavior.

use super::quota_urgency::{
    FIVE_HOUR_SECONDS, MONTHLY_SECONDS, WEEKLY_SECONDS, seconds_until, urgent_remaining,
};
use crate::worker_admin_types::{TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanWindowUsage};

/// Tightest CommandCode USD window after urgency lifting.
pub(crate) fn command_code_weight_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    [
        (key.five_hour.as_ref(), FIVE_HOUR_SECONDS),
        (key.weekly.as_ref(), WEEKLY_SECONDS),
    ]
    .into_iter()
    .filter_map(|(window, period)| {
        let window = window?;
        let remaining = window.remaining_percent?;
        Some(urgent_remaining(
            remaining,
            seconds_until(window.reset_at),
            period,
        ))
    })
    .min_by(|a, b| a.total_cmp(b))
}

/// Tightest OpencodeGo window after urgency lifting.
pub(crate) fn opencode_go_weight_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    [
        (key.opencodego_rolling.as_ref(), FIVE_HOUR_SECONDS),
        (key.opencodego_weekly.as_ref(), WEEKLY_SECONDS),
        (key.opencodego_monthly.as_ref(), MONTHLY_SECONDS),
    ]
    .into_iter()
    .filter_map(|(window, period)| {
        let window = window?;
        let remaining = 100.0 - window.percent?;
        Some(urgent_remaining(
            remaining,
            seconds_until(window.resets_at),
            period,
        ))
    })
    .min_by(|a, b| a.total_cmp(b))
}

/// Tightest GLM window after urgency lifting.
pub(crate) fn glm_weight_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    [
        (key.glm_five_hour.as_ref(), FIVE_HOUR_SECONDS),
        (key.glm_weekly.as_ref(), WEEKLY_SECONDS),
    ]
    .into_iter()
    .filter_map(|(window, period)| {
        let window = window?;
        let remaining = 100.0 - window.percentage?;
        Some(urgent_remaining(
            remaining,
            seconds_until(window.next_reset_at),
            period,
        ))
    })
    .min_by(|a, b| a.total_cmp(b))
}

fn window_weight_percent(window: &TokenPlanWindowUsage, period: f64) -> Option<f64> {
    let remaining = window.remaining_percent?;
    let resets_in = window
        .remains_time_ms
        .map(|ms| ms as f64 / 1000.0)
        .or_else(|| seconds_until(window.end_at));
    Some(urgent_remaining(remaining, resets_in, period))
}

/// Tightest of the MiniMax 5-hour/weekly token windows after urgency lifting.
pub(crate) fn effective_weight_percent(usage: &TokenPlanModelUsage) -> Option<f64> {
    let interval = usage
        .interval
        .as_ref()
        .and_then(|window| window_weight_percent(window, FIVE_HOUR_SECONDS));
    let weekly = usage
        .weekly
        .as_ref()
        .and_then(|window| window_weight_percent(window, WEEKLY_SECONDS));
    match (interval, weekly) {
        (Some(interval), Some(weekly)) => Some(interval.min(weekly).clamp(0.0, 100.0)),
        (Some(remaining), None) | (None, Some(remaining)) => Some(remaining.clamp(0.0, 100.0)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::{OpencodeGoWindowUsage, TokenPlanWindowUsage};
    use chrono::Duration;
    use uuid::Uuid;

    fn opencode_go_key(
        rolling_used: f64,
        rolling_reset: chrono::DateTime<chrono::Utc>,
        weekly: Option<(f64, chrono::DateTime<chrono::Utc>)>,
        monthly: Option<(f64, chrono::DateTime<chrono::Utc>)>,
    ) -> TokenPlanKeyUsage {
        let window = |used: f64, resets_at: chrono::DateTime<chrono::Utc>| OpencodeGoWindowUsage {
            status: None,
            percent: Some(used),
            resets_at: Some(resets_at),
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
            opencodego_rolling: Some(window(rolling_used, rolling_reset)),
            opencodego_weekly: weekly.map(|(used, reset)| window(used, reset)),
            opencodego_monthly: monthly.map(|(used, reset)| window(used, reset)),
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
        }
    }

    #[test]
    fn opencode_go_urgency_beats_the_raw_min_when_a_bottleneck_resets_soon() {
        // X: 3% left on the rolling window but it resets in 30 minutes;
        // monthly is nearly full. Y: 90% rolling but a weekly window at 6%
        // that resets in 6 days. Raw min favors Y (6 > 3); urgency favors X.
        let now = chrono::Utc::now();
        let x = opencode_go_key(
            97.0,
            now + Duration::minutes(30),
            None,
            Some((5.0, now + Duration::days(29))),
        );
        let y = opencode_go_key(
            10.0,
            now + Duration::hours(4),
            Some((94.0, now + Duration::days(6))),
            None,
        );
        let raw = |key: &TokenPlanKeyUsage| {
            // The parser helper stays the raw min; compare through it so the
            // assertion documents the behavior the urgency factor changes.
            super::super::opencode_go_parsing::opencode_go_remaining_percent(key).unwrap()
        };
        assert!(raw(&x) < raw(&y), "raw min must favor Y");
        let x_weight = opencode_go_weight_percent(&x).unwrap();
        let y_weight = opencode_go_weight_percent(&y).unwrap();
        assert!(
            x_weight > y_weight,
            "urgency must favor the fast-resetting X: {x_weight} vs {y_weight}"
        );
    }

    #[test]
    fn opencode_go_weight_without_reset_data_equals_the_raw_min() {
        let window = |percent: f64| OpencodeGoWindowUsage {
            status: None,
            percent: Some(percent),
            resets_at: None,
        };
        let mut key = opencode_go_key(
            35.0,
            chrono::Utc::now(),
            Some((75.0, chrono::Utc::now())),
            None,
        );
        key.opencodego_rolling = Some(window(35.0));
        key.opencodego_weekly = Some(window(75.0));
        key.opencodego_monthly = Some(window(90.0));
        assert_eq!(opencode_go_weight_percent(&key), Some(10.0));
    }

    #[test]
    fn minimax_windows_use_urgency_only_when_reset_data_exists() {
        let window = |remaining: f64| TokenPlanWindowUsage {
            status: Some(1),
            remaining_percent: Some(remaining),
            total_count: None,
            usage_count: None,
            boost_permille: None,
            start_at: None,
            end_at: None,
            remains_time_ms: None,
        };
        let mut urgent = TokenPlanModelUsage {
            model_name: "general".into(),
            interval: Some(window(10.0)),
            weekly: None,
        };
        urgent.interval.as_mut().unwrap().remains_time_ms = Some(5 * 60 * 1_000);
        assert!(effective_weight_percent(&urgent).unwrap() > 10.0);
        let raw = TokenPlanModelUsage {
            model_name: "general".into(),
            interval: Some(window(10.0)),
            weekly: None,
        };
        assert_eq!(effective_weight_percent(&raw), Some(10.0));
    }
}
