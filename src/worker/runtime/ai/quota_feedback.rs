mod retry_after;
#[cfg(test)]
#[path = "quota_feedback/tests.rs"]
mod tests;

use crate::{
    db::{EndpointProvider, RouteConfig},
    upstream_error::UpstreamFailureCause,
    worker::runtime::{context::RuntimeServices, routing::QuotaProbePermit},
    worker_admin::{
        quota_feedback::{QuotaFeedbackCause, QuotaFeedbackScope, QuotaFeedbackStore},
        token_plan_cache::TokenPlanQuotaCache,
    },
};
use http::HeaderMap;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(in crate::worker::runtime) struct AttemptFeedbackFactory {
    store: Option<QuotaFeedbackStore>,
    scope: QuotaFeedbackScope,
    probe: Option<QuotaProbePermit>,
}

impl AttemptFeedbackFactory {
    pub(in crate::worker::runtime) fn new(
        cache: Option<&TokenPlanQuotaCache>,
        route: &RouteConfig,
        probe: Option<QuotaProbePermit>,
    ) -> Self {
        Self {
            store: cache.map(|cache| cache.feedback.clone()),
            scope: QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id),
            probe,
        }
    }

    pub(in crate::worker::runtime) fn begin_attempt(&self) -> AttemptFeedback {
        self.begin_attempt_at(Instant::now())
    }

    fn begin_attempt_at(&self, attempt_start: Instant) -> AttemptFeedback {
        AttemptFeedback {
            store: self.store.clone(),
            scope: self.scope,
            attempt_start,
            probe: self.probe.clone(),
            settled: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone)]
pub(in crate::worker::runtime) struct AttemptFeedback {
    store: Option<QuotaFeedbackStore>,
    scope: QuotaFeedbackScope,
    attempt_start: Instant,
    probe: Option<QuotaProbePermit>,
    settled: Arc<AtomicBool>,
}

impl std::fmt::Debug for AttemptFeedback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttemptFeedback")
            .finish_non_exhaustive()
    }
}

impl AttemptFeedback {
    /// A 429 whose body could not be read: the block is still recorded, as a
    /// generic rate limit rather than a lost protection.
    pub(in crate::worker::runtime) fn record_429(&self, retry_after: Option<Duration>) {
        if !self.settled.swap(true, Ordering::AcqRel) {
            if let Some(store) = &self.store {
                store.record_429(self.scope, retry_after, Instant::now(), self.attempt_start);
            }
            self.release_probe();
        }
    }

    pub(in crate::worker::runtime) fn record_failure(
        &self,
        retry_after: Option<Duration>,
        cause: UpstreamFailureCause,
    ) {
        if !self.settled.swap(true, Ordering::AcqRel) {
            if let Some(store) = &self.store {
                store.record_failure(
                    self.scope,
                    feedback_cause(cause),
                    retry_after,
                    Instant::now(),
                    self.attempt_start,
                );
            }
            self.release_probe();
        }
    }

    pub(in crate::worker::runtime) fn completed_success(&self) {
        if !self.settled.swap(true, Ordering::AcqRel) {
            if let Some(store) = &self.store {
                store.record_success(self.scope, self.attempt_start, Instant::now());
            }
            self.release_probe();
        }
    }

    pub(in crate::worker::runtime) fn release_probe(&self) {
        if let Some(probe) = &self.probe {
            probe.release();
        }
    }
}

pub(in crate::worker::runtime) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    retry_after::parse(headers, chrono::Utc::now())
}

fn feedback_cause(cause: UpstreamFailureCause) -> QuotaFeedbackCause {
    match cause {
        UpstreamFailureCause::RateLimited => QuotaFeedbackCause::RateLimited,
        UpstreamFailureCause::UpstreamUnavailable => QuotaFeedbackCause::UpstreamUnavailable,
        UpstreamFailureCause::Exhausted => QuotaFeedbackCause::Exhausted,
    }
}

pub(in crate::worker::runtime) fn schedule_quota_refresh(
    services: &RuntimeServices,
    route: &RouteConfig,
) {
    if route.provider == EndpointProvider::OpenAi {
        if let Some(service) = services
            .admin_state()
            .and_then(|state| state.chatgpt_quota_service.as_deref())
        {
            service.notify_quota_error(route.route_id);
        }
        return;
    }
    if !matches!(
        route.provider,
        EndpointProvider::Minimax
            | EndpointProvider::CommandCode
            | EndpointProvider::OpencodeGo
            | EndpointProvider::OpenRouter
            | EndpointProvider::Glm
            | EndpointProvider::DeepSeek
    ) {
        return;
    }
    if let Some(state) = services.admin_state() {
        state
            .token_plan_quota
            .refresh_in_background(state.pool.clone(), route.route_id);
    }
}
