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
pub(crate) struct QuotaFeedbackStore(Arc<Mutex<FeedbackState>>);

/// The process-local feedback entries and the probe guards that are still
/// alive.
///
/// A guarded probe owns a live token for its whole in-flight lifetime, so an
/// eligibility read never expires it however long the upstream attempt takes.
/// The unguarded legacy reservation keeps the time-based recovery: its token
/// is not registered here.
#[derive(Default)]
struct FeedbackState {
    feedback: HashMap<QuotaFeedbackScope, Feedback>,
    live_probes: HashMap<QuotaFeedbackScope, Vec<Instant>>,
}

impl FeedbackState {
    fn has_live_probe(&self, scope: QuotaFeedbackScope) -> bool {
        self.live_probes
            .get(&scope)
            .is_some_and(|tokens| !tokens.is_empty())
    }

    fn register_live_probe(&mut self, scope: QuotaFeedbackScope, token: Instant) {
        self.live_probes.entry(scope).or_default().push(token);
    }

    fn unregister_live_probe(&mut self, scope: QuotaFeedbackScope, token: Instant) {
        let remove_entry = match self.live_probes.get_mut(&scope) {
            Some(tokens) => {
                if let Some(position) = tokens.iter().position(|live| *live == token) {
                    tokens.remove(position);
                }
                tokens.is_empty()
            }
            None => false,
        };
        if remove_entry {
            self.live_probes.remove(&scope);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaFeedbackEligibility {
    Eligible,
    Cooling,
    ProbeRequired,
    ProbeBusy,
}

/// Why a scope is blocked. Every cause shares one backoff clock, one probe
/// lifecycle and one routing effect; the cause only separates "the account is
/// spent" from "this was throttling" and "the upstream is unavailable" so the
/// badge and the API stop claiming an exhausted quota for a transient blip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaFeedbackCause {
    RateLimited,
    UpstreamUnavailable,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaProbeReservation {
    NotRequired,
    Reserved,
    Busy,
}

/// The outcome of claiming the single probe a scope may run at a time.
pub(crate) enum QuotaProbeClaim {
    /// The scope needs no probe, so the caller proceeds unguarded.
    NotRequired,
    /// The caller owns the scope's probe until it settles or is dropped.
    Reserved(QuotaProbeGuard),
    /// Another caller already probes this scope.
    Busy,
}

/// One in-flight probe on a scope.
///
/// The guard owns the reservation: it is released when the attempt settles
/// and when the guard is dropped, so a cancelled attempt never leaves a scope
/// permanently busy and never admits a parallel probe.
pub(crate) struct QuotaProbeGuard {
    store: QuotaFeedbackStore,
    scope: QuotaFeedbackScope,
    attempt_start: Instant,
    released: bool,
}

impl QuotaProbeGuard {
    pub(crate) fn release(&mut self) {
        if !self.released {
            self.released = true;
            self.store
                .release_probe(self.scope, self.attempt_start, Instant::now());
        }
    }
}

impl Drop for QuotaProbeGuard {
    fn drop(&mut self) {
        self.release();
    }
}

const RETENTION: Duration = Duration::from_secs(7200);
const PROBE_TTL: Duration = Duration::from_secs(30);

impl QuotaFeedbackStore {
    fn with<T>(&self, now: Instant, f: impl FnOnce(&mut FeedbackState) -> T) -> T {
        let mut state = self.0.lock().expect("quota feedback lock is not poisoned");
        state
            .feedback
            .retain(|_, entry| now.saturating_duration_since(entry.touched) < RETENTION);
        f(&mut state)
    }

    pub(crate) fn eligibility(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> QuotaFeedbackEligibility {
        self.with(now, |state| {
            let live = state.has_live_probe(scope);
            state
                .feedback
                .get(&scope)
                .map(|entry| entry.eligibility(now, live))
                .unwrap_or(QuotaFeedbackEligibility::Eligible)
        })
    }

    /// The active block with its cause, for the API projection. Read once so a
    /// state cannot change between the cause and the deadline the caller
    /// reports.
    pub(crate) fn active_cooldown(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> Option<(QuotaFeedbackCause, Duration)> {
        let state = self.0.lock().ok()?;
        let entry = state.feedback.get(&scope)?;
        if now.saturating_duration_since(entry.touched) >= RETENTION {
            return None;
        }
        entry
            .remaining_until(now)
            .map(|remaining| (entry.cause, remaining))
    }

    /// Reserve the scope's probe slot without owning a guard.
    ///
    /// This is the legacy reservation of the AI routing permit: it keeps the
    /// time-based recovery, so an abandoned reservation expires after
    /// [`PROBE_TTL`].
    pub(crate) fn reserve_probe_if_required(
        &self,
        scope: QuotaFeedbackScope,
        now: Instant,
    ) -> QuotaProbeReservation {
        self.with(now, |state| {
            let live = state.has_live_probe(scope);
            let Some(entry) = state.feedback.get(&scope) else {
                return QuotaProbeReservation::NotRequired;
            };
            match entry.eligibility(now, live) {
                QuotaFeedbackEligibility::Eligible => QuotaProbeReservation::NotRequired,
                QuotaFeedbackEligibility::ProbeRequired => {
                    let entry = state
                        .feedback
                        .get_mut(&scope)
                        .expect("the feedback entry just read is present");
                    entry.probe = Some(now);
                    entry.touched = now;
                    QuotaProbeReservation::Reserved
                }
                QuotaFeedbackEligibility::Cooling | QuotaFeedbackEligibility::ProbeBusy => {
                    QuotaProbeReservation::Busy
                }
            }
        })
    }

    /// Claim the scope's single probe slot.
    ///
    /// `NotRequired` means the scope has no probe to run, `Reserved` hands
    /// the caller the guard that owns the slot, and `Busy` means another
    /// caller holds it.
    pub(crate) fn claim_probe(&self, scope: QuotaFeedbackScope, now: Instant) -> QuotaProbeClaim {
        let reservation = self.with(now, |state| {
            let live = state.has_live_probe(scope);
            let decision = match state.feedback.get(&scope) {
                None => QuotaProbeReservation::NotRequired,
                Some(entry) => match entry.eligibility(now, live) {
                    QuotaFeedbackEligibility::Eligible => QuotaProbeReservation::NotRequired,
                    QuotaFeedbackEligibility::ProbeRequired => QuotaProbeReservation::Reserved,
                    QuotaFeedbackEligibility::Cooling | QuotaFeedbackEligibility::ProbeBusy => {
                        QuotaProbeReservation::Busy
                    }
                },
            };
            if matches!(decision, QuotaProbeReservation::Reserved) {
                if let Some(entry) = state.feedback.get_mut(&scope) {
                    entry.probe = Some(now);
                    entry.touched = now;
                }
                // The guard owns this token until it settles or is dropped.
                state.register_live_probe(scope, now);
            }
            decision
        });
        match reservation {
            QuotaProbeReservation::NotRequired => QuotaProbeClaim::NotRequired,
            QuotaProbeReservation::Reserved => QuotaProbeClaim::Reserved(QuotaProbeGuard {
                store: self.clone(),
                scope,
                attempt_start: now,
                released: false,
            }),
            QuotaProbeReservation::Busy => QuotaProbeClaim::Busy,
        }
    }

    #[cfg(test)]
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
        self.with(now, |state| {
            if let Some(entry) = state.feedback.get_mut(&scope)
                && entry.probe == Some(attempt_start)
            {
                entry.probe = None;
            }
            // Only the guard that registered this token removes it, so a
            // newer successor claim is never released by an older one.
            state.unregister_live_probe(scope, attempt_start);
        });
    }

    pub(crate) fn record_429(
        &self,
        scope: QuotaFeedbackScope,
        retry_after: Option<Duration>,
        now: Instant,
        attempt_start: Instant,
    ) {
        self.record_failure(
            scope,
            QuotaFeedbackCause::RateLimited,
            retry_after,
            now,
            attempt_start,
        );
    }

    pub(crate) fn record_failure(
        &self,
        scope: QuotaFeedbackScope,
        cause: QuotaFeedbackCause,
        retry_after: Option<Duration>,
        now: Instant,
        attempt_start: Instant,
    ) {
        self.with(now, |state| {
            let entry = state
                .feedback
                .entry(scope)
                .or_insert_with(|| Feedback::new(now, cause));
            entry.fail(cause, retry_after, now, attempt_start);
        });
    }

    pub(crate) fn record_success(
        &self,
        scope: QuotaFeedbackScope,
        attempt_start: Instant,
        now: Instant,
    ) {
        self.with(now, |state| {
            if state
                .feedback
                .get(&scope)
                .is_some_and(|entry| entry.failed_at <= attempt_start)
            {
                state.feedback.remove(&scope);
            }
        });
    }
}
