use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::db::ChatgptQuotaSnapshotSource;

pub(super) const ACTIVE_REFRESH_AFTER: ChronoDuration = ChronoDuration::minutes(5);
pub(super) const IDLE_REFRESH_AFTER: ChronoDuration = ChronoDuration::hours(1);
pub(super) const ACTIVITY_LOOKBACK: ChronoDuration = ChronoDuration::minutes(30);
pub(super) const FAILURE_BACKOFF_BASE_SECONDS: i64 = 60;
pub(super) const FAILURE_BACKOFF_MAX_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshTrigger {
    Manual,
    Request,
    Periodic,
}

impl RefreshTrigger {
    pub(super) const fn source(self) -> ChatgptQuotaSnapshotSource {
        match self {
            Self::Manual => ChatgptQuotaSnapshotSource::Manual,
            Self::Request => ChatgptQuotaSnapshotSource::Request,
            Self::Periodic => ChatgptQuotaSnapshotSource::Periodic,
        }
    }

    pub(super) const fn is_manual(self) -> bool {
        matches!(self, Self::Manual)
    }
}

pub(super) fn stale(observed_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(observed_at) > ACTIVE_REFRESH_AFTER
}

pub(super) fn periodic_due(
    last_success_at: Option<DateTime<Utc>>,
    active: bool,
    now: DateTime<Utc>,
) -> bool {
    let Some(last_success_at) = last_success_at else {
        return true;
    };
    let cadence = if active {
        ACTIVE_REFRESH_AFTER
    } else {
        IDLE_REFRESH_AFTER
    };
    now.signed_duration_since(last_success_at) >= cadence
}

pub(super) fn retry_delay(consecutive_failures: i32) -> ChronoDuration {
    let exponent = consecutive_failures.saturating_sub(1).clamp(0, 31) as u32;
    let seconds = FAILURE_BACKOFF_BASE_SECONDS
        .saturating_mul(1_i64.checked_shl(exponent).unwrap_or(i64::MAX))
        .min(FAILURE_BACKOFF_MAX_SECONDS);
    ChronoDuration::seconds(seconds)
}
