use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::token_plan_weight;
use crate::{db, worker_admin_types::TokenPlanUsageResponse};

const REFRESH_AFTER: Duration = Duration::from_secs(60);

/// Outstanding reservations for one (endpoint, key): the estimated tokens
/// and the number of draws that produced them. Percent-only windows damp by
/// draw count, MiniMax token windows convert the token share.
#[derive(Default, Clone, Copy)]
struct ReservationState {
    tokens: u64,
    draws: u64,
}

#[derive(Clone)]
pub(crate) struct TokenPlanQuotaCache {
    entries: Arc<RwLock<HashMap<Uuid, CachedUsage>>>,
    refresh_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
    reservations: Arc<std::sync::Mutex<HashMap<(Uuid, Uuid), ReservationState>>>,
}

#[derive(Clone)]
struct CachedUsage {
    usage: TokenPlanUsageResponse,
    fetched_at: Instant,
}

impl Default for TokenPlanQuotaCache {
    fn default() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
            reservations: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl TokenPlanQuotaCache {
    pub(crate) async fn refresh_if_due(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
    ) -> Result<Option<TokenPlanUsageResponse>> {
        // Both freshness checks read the entry once: the TTL verdict and the
        // payload come from the same guarded read, so a concurrent refresh
        // cannot answer one half of the question. The double check around the
        // per-endpoint refresh lock is unchanged, so a request that queued
        // behind an in-flight refresh still serves the fresh snapshot.
        if let Some(usage) = self.fresh_snapshot(endpoint_id).await {
            return Ok(Some(usage));
        }

        let lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks
                .entry(endpoint_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        if let Some(usage) = self.fresh_snapshot(endpoint_id).await {
            return Ok(Some(usage));
        }

        let Some(endpoint) = db::get_endpoint(pool, endpoint_id).await? else {
            return Ok(None);
        };
        if !matches!(
            endpoint.provider,
            db::EndpointProvider::Minimax
                | db::EndpointProvider::CommandCode
                | db::EndpointProvider::OpencodeGo
                | db::EndpointProvider::OpenRouter
                | db::EndpointProvider::Glm
                | db::EndpointProvider::DeepSeek
        ) {
            return Ok(None);
        }

        let usage = super::token_plan::fetch_endpoint_usage(&endpoint).await?;
        self.entries.write().await.insert(
            endpoint_id,
            CachedUsage {
                usage: usage.clone(),
                fetched_at: Instant::now(),
            },
        );
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| *id != endpoint_id);
        Ok(Some(usage))
    }

    pub(crate) async fn invalidate(&self, endpoint_id: Uuid) {
        self.entries.write().await.remove(&endpoint_id);
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| *id != endpoint_id);
    }

    /// One guarded read that answers "is there a snapshot inside the TTL, and
    /// what is it" together, so a caller never checks freshness and then reads
    /// the payload through a second lock acquisition that a concurrent refresh
    /// could have replaced in between.
    pub(crate) async fn fresh_snapshot(&self, endpoint_id: Uuid) -> Option<TokenPlanUsageResponse> {
        let entries = self.entries.read().await;
        let entry = entries.get(&endpoint_id)?;
        (entry.fetched_at.elapsed() < REFRESH_AFTER).then(|| entry.usage.clone())
    }

    /// Raw (urgency-free) remaining percent. Exhaustion checks use this so a
    /// depleted window is never resurrected just because it resets soon.
    pub(crate) fn key_remaining_percent_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
    ) -> Option<f64> {
        let snapshot = self.entries.try_read().ok()?;
        let key = snapshot
            .get(&endpoint_id)?
            .usage
            .keys
            .iter()
            .find(|key| key.key_id == key_id && key.ok)?;
        if let Some(remaining) = token_plan_weight::provider_remaining_percent(key) {
            return Some(remaining);
        }
        let usage = token_plan_weight::model_usage(key, model)?;
        let reserved_tokens = self.reservation_state(endpoint_id, key_id).0;
        token_plan_weight::model_remaining_percent(usage, reserved_tokens)
    }

    /// Pool weight: remaining percent lifted by reset urgency, then damped by
    /// outstanding reservations so a concurrent burst does not overshoot the
    /// same key. Always clamped to a finite non-negative percent.
    pub(crate) fn key_weight_percent_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
    ) -> Option<f64> {
        let snapshot = self.entries.try_read().ok()?;
        let key = snapshot
            .get(&endpoint_id)?
            .usage
            .keys
            .iter()
            .find(|key| key.key_id == key_id && key.ok)?;
        let (reserved_tokens, draws) = self.reservation_state(endpoint_id, key_id);
        if let Some(remaining) = token_plan_weight::provider_weight_percent(key) {
            return Some(token_plan_weight::apply_reservation_backpressure(
                remaining, draws,
            ));
        }
        let usage = token_plan_weight::model_usage(key, model)?;
        token_plan_weight::model_weight_percent(usage, reserved_tokens)
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
        self.entries.write().await.insert(
            endpoint_id,
            CachedUsage {
                usage,
                fetched_at: Instant::now(),
            },
        );
    }
}

pub(crate) fn estimate_input_tokens(body: &[u8]) -> u64 {
    let chars = String::from_utf8_lossy(body).chars().count() as u64;
    chars.saturating_add(3).saturating_div(4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::{
        OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse,
    };

    #[test]
    fn local_estimator_is_conservative_for_small_payloads() {
        assert_eq!(estimate_input_tokens(br#"{"model":"MiniMax-M3"}"#), 6);
        assert!(estimate_input_tokens("中文请求".as_bytes()) >= 1);
    }

    fn opencode_go_usage(key_id: Uuid, remaining: f64) -> TokenPlanUsageResponse {
        TokenPlanUsageResponse {
            local_today_tokens: None,
            provider: db::EndpointProvider::OpencodeGo,
            provider_region: None,
            keys: vec![TokenPlanKeyUsage {
                key_id,
                key_label: "k".into(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                model_remains: Vec::new(),
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: Some(OpencodeGoWindowUsage {
                    status: None,
                    percent: Some(100.0 - remaining),
                    resets_at: None,
                }),
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
            }],
        }
    }

    #[tokio::test]
    async fn reservations_damp_percent_windows_but_never_starve_them() {
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        let other_key_id = Uuid::new_v4();
        cache
            .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
            .await;

        assert_eq!(
            cache.key_weight_percent_now(endpoint_id, key_id, None),
            Some(80.0)
        );
        for _ in 0..5 {
            cache.reserve_estimated_tokens(endpoint_id, key_id, 1_000);
        }
        let damped = cache
            .key_weight_percent_now(endpoint_id, key_id, None)
            .expect("weighted key");
        assert!(damped < 80.0, "reservations must damp the weight: {damped}");
        assert!(damped > 0.0);

        for _ in 0..1_000_000 {
            cache.reserve_estimated_tokens(endpoint_id, key_id, 1);
        }
        let floor = cache
            .key_weight_percent_now(endpoint_id, key_id, None)
            .expect("weighted key");
        assert!(floor.is_finite() && floor > 0.0, "floor={floor}");
        // A key with no outstanding reservations is unaffected.
        assert_eq!(
            cache.key_weight_percent_now(endpoint_id, other_key_id, None),
            None
        );
    }

    #[tokio::test]
    async fn raw_remaining_ignores_urgency_and_reservations() {
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        cache
            .store_for_test(endpoint_id, opencode_go_usage(key_id, 30.0))
            .await;
        for _ in 0..50 {
            cache.reserve_estimated_tokens(endpoint_id, key_id, 10_000);
        }
        assert_eq!(
            cache.key_remaining_percent_now(endpoint_id, key_id, None),
            Some(30.0),
            "exhaustion checks must keep the raw window percent"
        );
    }

    #[tokio::test]
    async fn invalidate_drops_freshness_so_next_refresh_refetches() {
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        cache
            .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
            .await;
        assert!(
            cache
                .fresh_snapshot(endpoint_id)
                .await
                .expect("stored snapshot is fresh")
                .keys[0]
                .key_id
                == key_id
        );

        cache.invalidate(endpoint_id).await;

        // `refresh_if_due` short-circuits on a fresh snapshot. An invalidated
        // entry must stop being fresh so the next `refresh_candidate_quota` call
        // refetches instead of serving the pre-exhaustion snapshot (issue #310).
        assert!(cache.fresh_snapshot(endpoint_id).await.is_none());
    }

    #[tokio::test]
    async fn fresh_snapshot_serves_the_stored_payload_for_its_own_endpoint_only() {
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let other_endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        cache
            .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
            .await;

        // The single read returns the cached payload unchanged (same key, same
        // window), not a rebuilt or defaulted one.
        let served = cache
            .fresh_snapshot(endpoint_id)
            .await
            .expect("fresh entry");
        assert_eq!(served.keys.len(), 1);
        assert_eq!(served.keys[0].key_id, key_id);
        assert_eq!(
            served.keys[0]
                .opencodego_rolling
                .as_ref()
                .and_then(|window| window.percent),
            Some(20.0)
        );
        // A cold endpoint still reads as absent, so a fresh hit and an empty
        // cache stay distinguishable by one call.
        assert!(cache.fresh_snapshot(other_endpoint_id).await.is_none());
    }
}
