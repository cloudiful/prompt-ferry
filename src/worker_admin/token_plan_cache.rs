use super::{quota_feedback::QuotaFeedbackStore, token_plan_weight};
use crate::worker_admin_types::TokenPlanUsageResponse;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

mod observation;
mod refresh;
#[cfg(test)]
mod refresh_tests;
mod subscription;
#[cfg(test)]
mod tests;

const REFRESH_AFTER: Duration = Duration::from_secs(60);
const MIN_ATTEMPT: Duration = Duration::from_secs(10);

#[derive(Default, Clone, Copy)]
struct ReservationState {
    tokens: u64,
    draws: u64,
}

#[derive(Clone, Default)]
pub(crate) struct TokenPlanQuotaCache {
    entries: Arc<RwLock<HashMap<Uuid, CachedUsage>>>,
    refresh_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<refresh::RefreshState>>>>>,
    reservations: Arc<std::sync::Mutex<HashMap<(Uuid, Uuid), ReservationState>>>,
    subscription: Arc<
        std::sync::RwLock<HashMap<Uuid, super::chatgpt_quota_service::ChatGptQuotaRoutingSnapshot>>,
    >,
    pub(crate) feedback: QuotaFeedbackStore,
}

#[derive(Clone)]
struct CachedUsage {
    usage: TokenPlanUsageResponse,
    fetched_at: Instant,
    successful: HashMap<Uuid, Instant>,
    observed_at: HashMap<Uuid, chrono::DateTime<chrono::Utc>>,
    last_failure: HashMap<Uuid, Instant>,
}

impl TokenPlanQuotaCache {
    pub(crate) async fn refresh_if_due(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
        self.refresh(pool, endpoint_id, false).await
    }

    pub(crate) async fn invalidate(&self, endpoint_id: Uuid) {
        if let Some(entry) = self.entries.write().await.get_mut(&endpoint_id) {
            let now = Instant::now();
            for key in &entry.usage.keys {
                entry.last_failure.insert(key.key_id, now);
            }
        }
        self.clear_reservations(endpoint_id);
    }

    fn clear_reservations(&self, endpoint_id: Uuid) {
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| *id != endpoint_id);
    }

    pub(crate) async fn fresh_snapshot(&self, endpoint_id: Uuid) -> Option<TokenPlanUsageResponse> {
        let entries = self.entries.read().await;
        let entry = entries.get(&endpoint_id)?;
        let now = chrono::Utc::now();
        (entry.fetched_at.elapsed() < REFRESH_AFTER
            && entry.usage.keys.iter().all(|raw| {
                let mut key = raw.clone();
                observation::advance_relative_resets(&mut key, entry.fetched_at.elapsed());
                entry.successful.contains_key(&key.key_id)
                    && !entry.last_failure.contains_key(&key.key_id)
                    && !observation::expired_zero(&key, None, now)
                    && key
                        .model_remains
                        .iter()
                        .all(|model| !observation::expired_zero(&key, Some(&model.model_name), now))
            }))
        .then(|| entry.usage.clone())
    }

    pub(crate) fn quota_observation_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
    ) -> Option<observation::CachedQuotaObservation> {
        observation::quota_observation_now(self, endpoint_id, key_id, Instant::now())
    }

    pub(crate) fn key_remaining_percent_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
    ) -> Option<f64> {
        self.key_remaining_percent_scoped_now(endpoint_id, key_id, model, true)
    }

    pub(crate) fn key_remaining_percent_scoped_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
        allow_general: bool,
    ) -> Option<f64> {
        let key = self.fresh_observed_key_now(endpoint_id, key_id)?;
        if observation::expired_zero_scoped(&key, model, chrono::Utc::now(), allow_general) {
            return None;
        }
        if let Some(remaining) = token_plan_weight::provider_remaining_percent(&key) {
            return Some(remaining);
        }
        let usage = token_plan_weight::model_usage_scoped(&key, model, allow_general)?;
        token_plan_weight::model_remaining_percent(
            usage,
            self.reservation_state(endpoint_id, key_id).0,
        )
    }

    pub(crate) fn key_weight_percent_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
    ) -> Option<f64> {
        self.key_weight_percent_scoped_now(endpoint_id, key_id, model, true)
    }

    pub(crate) fn key_weight_percent_scoped_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
        allow_general: bool,
    ) -> Option<f64> {
        let key = self.fresh_observed_key_now(endpoint_id, key_id)?;
        if observation::expired_zero_scoped(&key, model, chrono::Utc::now(), allow_general) {
            return None;
        }
        let (tokens, draws) = self.reservation_state(endpoint_id, key_id);
        if let Some(remaining) = token_plan_weight::provider_remaining_percent(&key) {
            if remaining <= 0.0 {
                return Some(0.0);
            }
            return token_plan_weight::provider_weight_percent(&key)
                .map(|weight| token_plan_weight::apply_reservation_backpressure(weight, draws));
        }
        let usage = token_plan_weight::model_usage_scoped(&key, model, allow_general)?;
        if token_plan_weight::model_remaining_percent(usage, tokens) == Some(0.0) {
            return Some(0.0);
        }
        token_plan_weight::model_weight_percent(usage, tokens)
    }

    pub(crate) fn reserve_estimated_tokens(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        estimated_tokens: u64,
    ) {
        let mut reservations = self
            .reservations
            .lock()
            .expect("quota reservation lock is not poisoned");
        let entry = reservations.entry((endpoint_id, key_id)).or_default();
        entry.tokens = entry.tokens.saturating_add(estimated_tokens);
        entry.draws = entry.draws.saturating_add(1);
    }

    fn reservation_state(&self, endpoint_id: Uuid, key_id: Uuid) -> (u64, u64) {
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .get(&(endpoint_id, key_id))
            .map(|state| (state.tokens, state.draws))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) async fn store_for_test(&self, endpoint_id: Uuid, usage: TokenPlanUsageResponse) {
        self.store_observation(endpoint_id, usage, Instant::now())
            .await;
    }
}

pub(crate) fn estimate_input_tokens(body: &[u8]) -> u64 {
    let chars = String::from_utf8_lossy(body).chars().count() as u64;
    chars.saturating_add(3).saturating_div(4).max(1)
}
