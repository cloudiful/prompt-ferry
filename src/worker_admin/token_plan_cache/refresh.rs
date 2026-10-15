use super::*;
use crate::db;
use crate::db::QuotaObservationSource;
use chrono::Utc;
use std::future::Future;

#[derive(Default)]
pub(super) struct RefreshState {
    pub(super) attempted_at: Option<Instant>,
    pub(super) completed_at: Option<Instant>,
    background_pending: bool,
}

async fn fetch_live(
    pool: &PgPool,
    endpoint_id: Uuid,
) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
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
}

impl TokenPlanQuotaCache {
    pub(crate) async fn refresh(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
        force: bool,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
        self.refresh_with_source(
            pool,
            endpoint_id,
            force,
            QuotaObservationSource::Request,
            || fetch_live(pool, endpoint_id),
        )
        .await
    }

    /// Admin manual reads persist with the manual source; the fetch and the
    /// lock/reservation/error semantics match [`refresh`].
    pub(crate) async fn refresh_manual(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
        force: bool,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
        self.refresh_with_source(
            pool,
            endpoint_id,
            force,
            QuotaObservationSource::Manual,
            || fetch_live(pool, endpoint_id),
        )
        .await
    }

    /// Periodic sampler entry: the collector decides due-ness from the last
    /// persisted observation, so a due endpoint always forces a fresh fetch
    /// (which also retries a previously failed persist).
    pub(crate) async fn refresh_periodic(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>> {
        self.refresh_with_source(
            pool,
            endpoint_id,
            true,
            QuotaObservationSource::Periodic,
            || fetch_live(pool, endpoint_id),
        )
        .await
    }

    /// Fetch wrapper that persists every new result with its source and the
    /// timestamp captured at fetch completion. Cache hits and singleflight
    /// reuse return before the fetch closure runs, so they write no row.
    /// Persist failures log a constant message and never change the result.
    pub(crate) async fn refresh_with_source<F, Fut>(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
        force: bool,
        source: QuotaObservationSource,
        fetch: F,
    ) -> anyhow::Result<Option<TokenPlanUsageResponse>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<Option<TokenPlanUsageResponse>>>,
    {
        self.refresh_with(endpoint_id, force, Instant::now(), || async {
            match fetch().await {
                Ok(Some(mut response)) => {
                    let observed_at = Utc::now();
                    crate::worker_admin::quota_canonical::populate_response_observations(
                        response.provider,
                        &mut response,
                    );
                    crate::worker_admin::quota_observation_service::persist_response(
                        pool,
                        endpoint_id,
                        response.provider,
                        &response,
                        source,
                        observed_at,
                        None,
                        None,
                    )
                    .await;
                    Ok(Some(response))
                }
                Ok(None) => Ok(None),
                Err(error) => {
                    let observed_at = Utc::now();
                    if let Ok(Some(endpoint)) = db::get_endpoint(pool, endpoint_id).await {
                        let keys =
                            crate::worker_admin::quota_observation_service::enabled_key_identities(
                                &endpoint,
                            );
                        if !keys.is_empty() {
                            crate::worker_admin::quota_observation_service::persist_endpoint_failure(
                                pool,
                                endpoint_id,
                                endpoint.provider,
                                &keys,
                                source,
                                observed_at,
                            )
                            .await;
                        }
                    }
                    Err(error)
                }
            }
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
