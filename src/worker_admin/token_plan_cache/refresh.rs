use super::*;
use crate::db;
use std::future::Future;

#[derive(Default)]
pub(super) struct RefreshState {
    pub(super) attempted_at: Option<Instant>,
    pub(super) completed_at: Option<Instant>,
    background_pending: bool,
}

impl TokenPlanQuotaCache {
    pub(crate) async fn refresh(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
        force: bool,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
        self.refresh_with(endpoint_id, force, Instant::now(), || async {
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
            super::super::token_plan::fetch_endpoint_usage(&endpoint)
                .await
                .map(Some)
        })
        .await
    }

    pub(crate) fn refresh_in_background(&self, pool: PgPool, endpoint_id: Uuid) {
        let lock = {
            let Ok(mut locks) = self.refresh_locks.try_lock() else {
                return;
            };
            if !locks.contains_key(&endpoint_id) && locks.len() >= 4096 {
                return;
            }
            locks
                .entry(endpoint_id)
                .or_insert_with(|| Arc::new(Mutex::new(RefreshState::default())))
                .clone()
        };
        {
            let Ok(mut state) = lock.try_lock() else {
                return;
            };
            if state.background_pending
                || state
                    .attempted_at
                    .is_some_and(|at| at.elapsed() < MIN_ATTEMPT)
            {
                return;
            }
            state.background_pending = true;
        }
        let cache = self.clone();
        tokio::spawn(async move {
            let _ = cache.refresh(&pool, endpoint_id, true).await;
            lock.lock().await.background_pending = false;
        });
    }

    pub(super) async fn refresh_with<F, Fut>(
        &self,
        endpoint_id: Uuid,
        force: bool,
        arrived_at: Instant,
        fetch: F,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<Option<TokenPlanUsageResponse>>>,
    {
        if !force && let Some(usage) = self.fresh_snapshot(endpoint_id).await {
            return Ok(Some(usage));
        }
        let lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks.retain(|id, lock| {
                *id == endpoint_id
                    || Arc::strong_count(lock) > 1
                    || lock
                        .try_lock()
                        .map(|state| {
                            state.attempted_at.is_some_and(|at| {
                                arrived_at.saturating_duration_since(at) < REFRESH_AFTER
                            })
                        })
                        .unwrap_or(true)
            });
            if !locks.contains_key(&endpoint_id) && locks.len() >= 4096 {
                drop(locks);
                return Ok(self.raw_snapshot(endpoint_id).await);
            }
            locks
                .entry(endpoint_id)
                .or_insert_with(|| Arc::new(Mutex::new(RefreshState::default())))
                .clone()
        };
        let mut state = lock.lock().await;
        if state.completed_at.is_some_and(|at| at >= arrived_at)
            || state.attempted_at.is_some_and(|at| {
                at >= arrived_at || arrived_at.saturating_duration_since(at) < MIN_ATTEMPT
            })
        {
            return Ok(self.raw_snapshot(endpoint_id).await);
        }
        if !force && let Some(usage) = self.fresh_snapshot(endpoint_id).await {
            return Ok(Some(usage));
        }
        state.attempted_at = Some(arrived_at);
        let result = fetch().await;
        let result = match result {
            Ok(Some(usage)) => {
                self.store_observation(endpoint_id, usage, Instant::now())
                    .await;
                self.clear_reservations(endpoint_id);
                Ok(self.raw_snapshot(endpoint_id).await)
            }
            Ok(None) => {
                self.invalidate(endpoint_id).await;
                Ok(None)
            }
            Err(error) => {
                self.invalidate(endpoint_id).await;
                Err(error)
            }
        };
        state.completed_at = Some(Instant::now());
        result
    }
}
