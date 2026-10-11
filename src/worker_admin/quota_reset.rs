use chrono::{DateTime, Duration, Utc};

use crate::worker_admin_types::{
    SubscriptionWindowAvailability, SubscriptionWindowUsage, TokenPlanQuotaState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResetState {
    Passed,
    Future,
    Unknown,
}

pub(crate) fn reset_state(
    absolute: Option<DateTime<Utc>>,
    relative: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> ResetState {
    let absolute = deadline_state(absolute, now);
    let relative = deadline_state(relative, now);
    match (absolute, relative) {
        (ResetState::Passed, _) | (_, ResetState::Passed) => ResetState::Passed,
        (ResetState::Future, _) | (_, ResetState::Future) => ResetState::Future,
        _ => ResetState::Unknown,
    }
}

pub(crate) fn reset_state_from_remaining(
    absolute: Option<DateTime<Utc>>,
    remaining_time_ms: Option<i64>,
    now: DateTime<Utc>,
) -> ResetState {
    let relative = remaining_time_ms.and_then(|remaining| {
        if remaining <= 0 {
            Some(now)
        } else {
            now.checked_add_signed(Duration::milliseconds(remaining))
        }
    });
    reset_state(absolute, relative, now)
}

pub(crate) fn subscription_reset_state(
    window: &SubscriptionWindowUsage,
    observed_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> ResetState {
    let relative = window.reset_after_seconds.and_then(|seconds| {
        if seconds <= 0 {
            Some(now)
        } else {
            observed_at.checked_add_signed(Duration::seconds(seconds))
        }
    });
    reset_state(window.reset_at, relative, now)
}

pub(crate) fn subscription_window_is_exhausted(
    window: &SubscriptionWindowUsage,
    observed_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    window.availability == SubscriptionWindowAvailability::Known
        && window
            .remaining_percent
            .is_some_and(|remaining| remaining.is_finite() && remaining <= 0.0)
        && subscription_reset_state(window, observed_at, now) != ResetState::Passed
}

pub(crate) fn subscription_windows_all_reset(
    windows: &[SubscriptionWindowUsage],
    observed_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    !windows.is_empty()
        && windows.iter().all(|window| {
            window.availability == SubscriptionWindowAvailability::Known
                && subscription_reset_state(window, observed_at, now) == ResetState::Passed
        })
}

pub(crate) fn subscription_windows_state(
    windows: &[&SubscriptionWindowUsage],
    observed_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> TokenPlanQuotaState {
    if windows.is_empty() {
        return TokenPlanQuotaState::Unknown;
    }
    let mut exhausted = false;
    let mut unknown = false;
    let mut positive = false;
    for window in windows {
        if window.availability == SubscriptionWindowAvailability::Unknown
            || subscription_reset_state(window, observed_at, now) == ResetState::Passed
        {
            unknown = true;
            continue;
        }
        match window.remaining_percent.filter(|value| value.is_finite()) {
            Some(value) if value <= 0.0 => exhausted = true,
            Some(value) if value <= 100.0 => positive = true,
            _ => unknown = true,
        }
    }
    if exhausted {
        TokenPlanQuotaState::Exhausted
    } else if unknown || !positive {
        TokenPlanQuotaState::Unknown
    } else {
        TokenPlanQuotaState::Available
    }
}

fn deadline_state(deadline: Option<DateTime<Utc>>, now: DateTime<Utc>) -> ResetState {
    match deadline {
        Some(deadline) if deadline <= now => ResetState::Passed,
        Some(_) => ResetState::Future,
        None => ResetState::Unknown,
    }
}
