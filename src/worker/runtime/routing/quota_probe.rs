use crate::worker_admin::{
    quota_feedback::{QuotaFeedbackScope, QuotaFeedbackStore, QuotaProbeReservation},
    token_plan_cache::TokenPlanQuotaCache,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use uuid::Uuid;

#[derive(Clone)]
pub(in crate::worker::runtime) struct QuotaProbePermit(Arc<ProbeReservation>);

struct ProbeReservation {
    store: QuotaFeedbackStore,
    scope: QuotaFeedbackScope,
    reserved_at: Instant,
    active: AtomicBool,
}

impl QuotaProbePermit {
    fn new(store: QuotaFeedbackStore, scope: QuotaFeedbackScope, reserved_at: Instant) -> Self {
        Self(Arc::new(ProbeReservation {
            store,
            scope,
            reserved_at,
            active: AtomicBool::new(true),
        }))
    }

    pub(in crate::worker::runtime) fn release(&self) {
        if self.0.active.swap(false, Ordering::AcqRel) {
            self.0
                .store
                .release_probe(self.0.scope, self.0.reserved_at, Instant::now());
        }
    }
}

impl Drop for ProbeReservation {
    fn drop(&mut self) {
        if self.active.swap(false, Ordering::AcqRel) {
            self.store
                .release_probe(self.scope, self.reserved_at, Instant::now());
        }
    }
}

pub(in crate::worker::runtime) enum ProbeClaim {
    NotRequired,
    Reserved(QuotaProbePermit),
    LostRace,
}

pub(in crate::worker::runtime) fn claim(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint_id: Uuid,
    key_id: Option<Uuid>,
) -> ProbeClaim {
    let Some(cache) = cache else {
        return ProbeClaim::NotRequired;
    };
    claim_at(
        &cache.feedback,
        QuotaFeedbackScope::new(endpoint_id, key_id),
        Instant::now(),
    )
}

fn claim_at(store: &QuotaFeedbackStore, scope: QuotaFeedbackScope, now: Instant) -> ProbeClaim {
    match store.reserve_probe_if_required(scope, now) {
        QuotaProbeReservation::NotRequired => ProbeClaim::NotRequired,
        QuotaProbeReservation::Reserved => {
            ProbeClaim::Reserved(QuotaProbePermit::new(store.clone(), scope, now))
        }
        QuotaProbeReservation::Busy => ProbeClaim::LostRace,
    }
}

pub(super) fn candidate_claim_limit(candidate: &crate::db::ModelRouteCandidate) -> usize {
    candidate
        .targets
        .iter()
        .map(|target| target.api_keys.len().max(1))
        .sum::<usize>()
        .max(1)
}

pub(super) fn route_claim_limit(route: &crate::db::RouteConfig) -> usize {
    route.api_keys.len().max(1)
}

#[cfg(test)]
#[path = "quota_probe_tests.rs"]
mod tests;
