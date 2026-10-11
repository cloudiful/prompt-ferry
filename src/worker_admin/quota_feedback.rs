//! Process-local feedback; cloned caches share this store, not other replicas.
mod cooldown;
#[cfg(test)]
mod tests;

use cooldown::Feedback;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct QuotaFeedbackScope {
    pub endpoint_id: Uuid,
    pub key_id: Option<Uuid>,
}

impl QuotaFeedbackScope {
    pub(crate) fn new(endpoint_id: Uuid, key_id: Option<Uuid>) -> Self {
        Self {
            endpoint_id,
            key_id: key_id.filter(|id| !id.is_nil()),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct QuotaFeedbackStore(Arc<Mutex<HashMap<QuotaFeedbackScope, Feedback>>>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaFeedbackEligibility {
    Eligible,
    Cooling,
    ProbeRequired,
    ProbeBusy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaProbeReservation {
    NotRequired,
    Reserved,
    Busy,
}

const RETENTION: Duration = Duration::from_secs(7200);
const PROBE_TTL: Duration = Duration::from_secs(30);

impl QuotaFeedbackStore {
    fn with<T>(
        &self,
        now: Instant,
        f: impl FnOnce(&mut HashMap<QuotaFeedbackScope, Feedback>) -> T,
    ) -> T {
        let mut entries = self.0.lock().expect("quota feedback lock is not poisoned");
        entries.retain(|_, state| now.saturating_duration_since(state.touched) < RETENTION);
        f(&mut entries)
    }

    pub(crate) fn eligibility(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> QuotaFeedbackEligibility {
        self.with(now, |entries| {
            entries
                .get(&scope)
                .map(|state| state.eligibility(now))
                .unwrap_or(QuotaFeedbackEligibility::Eligible)
        })
    }

    pub(crate) fn remaining_cooldown(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> Option<Duration> {
        let entries = self.0.lock().ok()?;
        let state = entries.get(&scope)?;
        if now.saturating_duration_since(state.touched) >= RETENTION {
            return None;
        }
        state.remaining_until(now)
    }

    pub(crate) fn reserve_probe_if_required(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> QuotaProbeReservation {
        self.with(now, |entries| {
            let Some(state) = entries.get_mut(&scope) else {
                return QuotaProbeReservation::NotRequired;
            };
            match state.eligibility(now) {
                QuotaFeedbackEligibility::Eligible => QuotaProbeReservation::NotRequired,
                QuotaFeedbackEligibility::ProbeRequired => {
                    state.probe = Some(now);
                    state.touched = now;
                    QuotaProbeReservation::Reserved
                }
                QuotaFeedbackEligibility::Cooling | QuotaFeedbackEligibility::ProbeBusy => {
                    QuotaProbeReservation::Busy
                }
            }
        })
    }

    pub(crate) fn reserve_probe(&self, scope: QuotaFeedbackScope, now: Instant) -> bool {
        !matches!(
            self.reserve_probe_if_required(scope, now),
            QuotaProbeReservation::Busy
        )
    }

    pub(crate) fn release_probe(
        &self,
        scope: QuotaFeedbackScope,
        attempt_start: Instant,
        now: Instant,
    ) {
        self.with(now, |entries| {
            if let Some(state) = entries.get_mut(&scope) {
                if state.probe == Some(attempt_start) {
                    state.probe = None;
                }
            }
        });
    }

    pub(crate) fn record_429(
        &self,
        scope: QuotaFeedbackScope,
        retry_after: Option<Duration>,
        now: Instant,
        attempt_start: Instant,
    ) {
        self.with(now, |entries| {
            let state = entries.entry(scope).or_insert_with(|| Feedback::new(now));
            state.fail(retry_after, now, attempt_start);
        });
    }

    pub(crate) fn record_success(
        &self,
        scope: QuotaFeedbackScope,
        attempt_start: Instant,
        now: Instant,
    ) {
        self.with(now, |entries| {
            if entries
                .get(&scope)
                .is_some_and(|state| state.failed_at <= attempt_start)
            {
                entries.remove(&scope);
            }
        });
    }
}
