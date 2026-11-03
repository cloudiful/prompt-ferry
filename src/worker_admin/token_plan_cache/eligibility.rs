//! Domain-neutral quota assessment shared by the AI routing path and the
//! builtin MCP provider path (issue #1179 P1).
//!
//! The assessment is the reusable core of the AI eligibility check: it reads
//! the same cached quota snapshot, the same per-key remaining and weight
//! budgets and the same process-local feedback store, so a caller outside the
//! worker routing modules can reuse the quota signal without copying its
//! state machine. The worker keeps its own mapping as a thin wrapper over
//! this core.

use super::TokenPlanQuotaCache;
use crate::worker_admin::quota_feedback::{
    QuotaFeedbackEligibility, QuotaFeedbackScope, QuotaProbeClaim,
};
use std::time::Instant;
use uuid::Uuid;

/// The quota verdict for one credential scope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuotaAssessment {
    /// The scope may be used; the value is its draw weight.
    Eligible(f64),
    /// The scope is known to be spent.
    Exhausted,
    /// The scope is in a rate-limit or failure cooldown.
    Cooling,
    /// Another caller is probing the scope.
    ProbeBusy,
}

/// The scope a caller assesses.
///
/// `model` and `allow_general` select the quota group inside the cached
/// snapshot; `snapshot` decides whether the cached snapshot gates the scope
/// at all. A scope with `snapshot` disabled still honours the process-local
/// feedback store, so a key that just failed upstream stays out of the pool
/// even when no quota group is known for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuotaScope<'a> {
    pub endpoint_id: Uuid,
    pub key_id: Option<Uuid>,
    pub model: Option<&'a str>,
    pub allow_general: bool,
    pub snapshot: bool,
}

impl<'a> QuotaScope<'a> {
    pub(crate) fn new(
        endpoint_id: Uuid,
        key_id: Option<Uuid>,
        model: Option<&'a str>,
        allow_general: bool,
    ) -> Self {
        Self {
            endpoint_id,
            key_id,
            model,
            allow_general,
            snapshot: true,
        }
    }

    /// A scope that is not gated by the cached quota snapshot.
    ///
    /// The builtin image tool no longer assesses through the shared cache (its
    /// billing group is unknown), so this constructor now exists for the
    /// assessment-core tests that pin the no-snapshot behaviour.
    #[cfg(test)]
    pub(crate) fn without_snapshot(
        endpoint_id: Uuid,
        key_id: Option<Uuid>,
        model: Option<&'a str>,
        allow_general: bool,
    ) -> Self {
        Self {
            snapshot: false,
            ..Self::new(endpoint_id, key_id, model, allow_general)
        }
    }

    pub(crate) fn feedback_scope(self) -> QuotaFeedbackScope {
        QuotaFeedbackScope::new(self.endpoint_id, self.key_id)
    }
}

impl TokenPlanQuotaCache {
    /// Assess one credential scope.
    ///
    /// An unknown or stale snapshot is not a signal: the scope stays eligible
    /// with a neutral weight. Only a confirmed zero remaining budget, an
    /// active cooldown or a running probe removes a scope from the pool.
    pub(crate) fn assess_scope(&self, scope: QuotaScope<'_>) -> QuotaAssessment {
        match self
            .feedback
            .eligibility(scope.feedback_scope(), Instant::now())
        {
            QuotaFeedbackEligibility::Cooling => return QuotaAssessment::Cooling,
            QuotaFeedbackEligibility::ProbeBusy => return QuotaAssessment::ProbeBusy,
            QuotaFeedbackEligibility::Eligible | QuotaFeedbackEligibility::ProbeRequired => {}
        }
        if !scope.snapshot {
            return QuotaAssessment::Eligible(1.0);
        }
        if self.chatgpt_subscription_exhausted_now(scope.endpoint_id) {
            return QuotaAssessment::Exhausted;
        }
        let key = scope.key_id.unwrap_or(Uuid::nil());
        if self
            .key_remaining_percent_scoped_now(
                scope.endpoint_id,
                key,
                scope.model,
                scope.allow_general,
            )
            .is_some_and(|remaining| remaining.is_finite() && remaining <= 0.0)
        {
            return QuotaAssessment::Exhausted;
        }
        match self.key_weight_percent_scoped_now(
            scope.endpoint_id,
            key,
            scope.model,
            scope.allow_general,
        ) {
            Some(weight) if weight.is_finite() && weight <= 0.0 => QuotaAssessment::Exhausted,
            Some(weight) if weight.is_finite() => QuotaAssessment::Eligible(weight.min(100.0)),
            _ => QuotaAssessment::Eligible(1.0),
        }
    }

    /// Claim the scope's single probe slot, when it needs one.
    pub(crate) fn claim_scope_probe(&self, scope: QuotaScope<'_>) -> QuotaProbeClaim {
        self.feedback
            .claim_probe(scope.feedback_scope(), Instant::now())
    }
}

#[cfg(test)]
#[path = "eligibility_tests.rs"]
mod tests;
