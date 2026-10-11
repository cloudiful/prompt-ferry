use super::quota_unavailable::QuotaUnavailableError;
use crate::worker_admin::{
    quota_feedback::{QuotaFeedbackEligibility, QuotaFeedbackScope},
    token_plan_cache::TokenPlanQuotaCache,
};
use std::time::Instant;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) struct QuotaContext<'a> {
    pub model: Option<&'a str>,
    pub path: &'a str,
}

impl<'a> QuotaContext<'a> {
    pub fn effective(self, upstream_model: Option<&'a str>) -> Self {
        Self {
            model: upstream_model.or(self.model),
            ..self
        }
    }
    fn allow_general(self) -> bool {
        matches!(
            self.path,
            "/v1/messages" | "/v1/chat/completions" | "/v1/responses" | "/v1/responses/compact"
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Eligibility {
    Eligible(f64),
    Exhausted,
    Cooling,
    ProbeBusy,
}
impl Eligibility {
    pub fn weight(self) -> Option<f64> {
        match self {
            Self::Eligible(weight) => Some(weight),
            _ => None,
        }
    }
}

pub(super) fn assess(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint: Uuid,
    key: Option<Uuid>,
    context: QuotaContext<'_>,
) -> Eligibility {
    let Some(cache) = cache else {
        return Eligibility::Eligible(1.0);
    };
    match cache
        .feedback
        .eligibility(QuotaFeedbackScope::new(endpoint, key), Instant::now())
    {
        QuotaFeedbackEligibility::Cooling => return Eligibility::Cooling,
        QuotaFeedbackEligibility::ProbeBusy => return Eligibility::ProbeBusy,
        QuotaFeedbackEligibility::Eligible | QuotaFeedbackEligibility::ProbeRequired => {}
    }
    if cache.chatgpt_subscription_exhausted_now(endpoint) {
        return Eligibility::Exhausted;
    }
    let key = key.unwrap_or(Uuid::nil());
    if cache
        .key_remaining_percent_scoped_now(endpoint, key, context.model, context.allow_general())
        .is_some_and(|remaining| remaining.is_finite() && remaining <= 0.0)
    {
        return Eligibility::Exhausted;
    }
    match cache.key_weight_percent_scoped_now(endpoint, key, context.model, context.allow_general())
    {
        Some(weight) if weight.is_finite() && weight <= 0.0 => Eligibility::Exhausted,
        Some(weight) if weight.is_finite() => Eligibility::Eligible(weight.min(100.0)),
        _ => Eligibility::Eligible(1.0),
    }
}

pub(super) struct QuotaPool<T> {
    pub units: Vec<T>,
    pub configured: usize,
    pub blocked: usize,
}
impl<T> Default for QuotaPool<T> {
    fn default() -> Self {
        Self {
            units: Vec::new(),
            configured: 0,
            blocked: 0,
        }
    }
}
impl<T> QuotaPool<T> {
    pub fn consider(&mut self, eligibility: Eligibility, unit: impl FnOnce(f64) -> T) {
        self.configured += 1;
        match eligibility.weight() {
            Some(weight) => self.units.push(unit(weight)),
            None => self.blocked += 1,
        }
    }
    pub fn checked(&self) -> Result<(), QuotaUnavailableError> {
        if self.units.is_empty() && self.configured > 0 && self.blocked == self.configured {
            Err(QuotaUnavailableError)
        } else {
            Ok(())
        }
    }
    pub fn extend(&mut self, pool: Self) {
        self.units.extend(pool.units);
        self.configured += pool.configured;
        self.blocked += pool.blocked;
    }
}

pub(super) fn reserve(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint: Uuid,
    key: Option<Uuid>,
    estimated: u64,
) {
    if let Some(cache) = cache {
        cache.reserve_estimated_tokens(endpoint, key.unwrap_or(Uuid::nil()), estimated);
    }
}
