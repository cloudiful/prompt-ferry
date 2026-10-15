use std::{collections::HashMap, sync::Arc};

use futures::future::BoxFuture;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{db::ConfigRepository, worker_admin::handlers::chatgpt_backend::ChatgptQuota};

use super::{ChatGptQuotaError, ChatGptQuotaFetcher};

mod concurrency;
mod failure;
mod manual_waiting_diagnostics;
mod manual_waiting_failure;
mod policy;
mod refresh_cycle_tests;
mod routing_snapshot;
mod support;

pub(crate) fn fake_fetcher(quota: ChatgptQuota) -> FakeFetcher {
    FakeFetcher::success(quota)
}

pub(crate) struct FakeFetcher {
    pub calls: Arc<std::sync::atomic::AtomicUsize>,
    per_endpoint: Arc<std::sync::Mutex<HashMap<Uuid, usize>>>,
    result: Result<ChatgptQuota, ChatGptQuotaError>,
    started: Option<Arc<Notify>>,
    release: Option<Arc<FetchRelease>>,
}

pub(crate) struct FetchRelease {
    opened: std::sync::atomic::AtomicBool,
    notify: Notify,
}

impl FetchRelease {
    pub(crate) fn open(&self) {
        self.opened.store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        loop {
            if self.opened.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let notified = self.notify.notified();
            tokio::pin!(notified);
            if self.opened.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

impl FakeFetcher {
    pub(crate) fn success(quota: ChatgptQuota) -> Self {
        Self::new(Ok(quota))
    }

    pub(crate) fn failure(error: ChatGptQuotaError) -> Self {
        Self::new(Err(error))
    }

    pub(crate) fn gated(quota: ChatgptQuota) -> (Self, Arc<Notify>, Arc<FetchRelease>) {
        let started = Arc::new(Notify::new());
        let release = Arc::new(FetchRelease {
            opened: std::sync::atomic::AtomicBool::new(false),
            notify: Notify::new(),
        });
        let mut fetcher = Self::success(quota);
        fetcher.started = Some(started.clone());
        fetcher.release = Some(release.clone());
        (fetcher, started, release)
    }

    fn new(result: Result<ChatgptQuota, ChatGptQuotaError>) -> Self {
        Self {
            calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            per_endpoint: Arc::new(std::sync::Mutex::new(HashMap::new())),
            result,
            started: None,
            release: None,
        }
    }

    /// Exact fetch count for one endpoint. Foreign fixtures share the same
    /// global collector, so cadence assertions scope to owned ids through here
    /// instead of the global counter.
    pub(crate) fn calls_for(&self, endpoint_id: Uuid) -> usize {
        self.per_endpoint
            .lock()
            .expect("per-endpoint fetch counts stay lockable")
            .get(&endpoint_id)
            .copied()
            .unwrap_or(0)
    }

    /// Total fetches for endpoints outside `owned`: independent foreign calls
    /// are reported, never failed, so historical orphans stay visible.
    pub(crate) fn foreign_calls_excluding(&self, owned: &[Uuid]) -> usize {
        self.per_endpoint
            .lock()
            .expect("per-endpoint fetch counts stay lockable")
            .iter()
            .filter(|(endpoint_id, _)| !owned.contains(endpoint_id))
            .map(|(_, count)| count)
            .sum()
    }
}

impl ChatGptQuotaFetcher for FakeFetcher {
    fn fetch<'a>(
        &'a self,
        endpoint_id: Uuid,
        _repository: &'a ConfigRepository,
    ) -> BoxFuture<'a, Result<ChatgptQuota, ChatGptQuotaError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Recorded before any await so gated fetches count the endpoint
            // even while the release is still closed.
            *self
                .per_endpoint
                .lock()
                .expect("per-endpoint fetch counts stay lockable")
                .entry(endpoint_id)
                .or_insert(0) += 1;
            if let Some(started) = &self.started {
                started.notify_one();
            }
            if let Some(release) = &self.release {
                release.wait().await;
            }
            self.result.clone()
        })
    }
}

#[cfg(test)]
mod mirror_tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};

    use crate::db;

    use super::support::{
        cleanup_endpoint, create_endpoint, endpoint, quota, seed_snapshot, service,
    };
    use super::{FakeFetcher, fake_fetcher};

    #[tokio::test]
    async fn successful_refresh_mirrors_canonical_observation_without_extra_fetch() {
        let Some(pool) = super::support::test_pool().await else {
            return;
        };
        let endpoint_id = create_endpoint(&pool).await;
        let seeded_at = Utc::now() - Duration::minutes(8);
        seed_snapshot(&pool, endpoint_id, seeded_at, Some(30.0)).await;
        let fetcher = Arc::new(fake_fetcher(quota(Some(20.0))));
        let service = service(&pool, fetcher.clone());
        let provider_endpoint = endpoint(&pool, endpoint_id).await;

        let response = service
            .read(&provider_endpoint, true)
            .await
            .expect("manual refresh succeeds");
        assert!(response.keys[0].ok);
        // The mirror reuses the fetched quota: no second provider call.
        assert_eq!(fetcher.calls_for(endpoint_id), 1);
        // The legacy history stays authoritative and keeps both snapshots.
        let snapshots = db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
            .await
            .unwrap();
        assert_eq!(snapshots.len(), 2);
        // The mirror row carries the stable key id, the saved stamp, and the
        // manual source with canonical windows and plan metadata.
        let key_id = response.keys[0].key_id;
        let row = db::quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
            .await
            .unwrap()
            .expect("mirror row persists");
        assert!(row.ok);
        assert_eq!(row.provider, db::QuotaObservationProvider::Openai);
        assert_eq!(row.source, db::QuotaObservationSource::Manual);
        assert_eq!(
            row.observed_at.timestamp_micros(),
            snapshots[0].observed_at.timestamp_micros()
        );
        assert!(!row.windows.is_empty());
        assert_eq!(row.plan_type.as_deref(), Some("plus"));
        assert_eq!(row.limit_reached, Some(false));
        cleanup_endpoint(&pool, endpoint_id).await;
    }

    #[tokio::test]
    async fn failed_refresh_writes_no_mirror_row_and_keeps_old_history() {
        let Some(pool) = super::support::test_pool().await else {
            return;
        };
        let endpoint_id = create_endpoint(&pool).await;
        let seeded_at = Utc::now() - Duration::minutes(8);
        seed_snapshot(&pool, endpoint_id, seeded_at, Some(30.0)).await;
        let fetcher = Arc::new(FakeFetcher::failure(super::ChatGptQuotaError::Upstream));
        let service = service(&pool, fetcher.clone());
        let provider_endpoint = endpoint(&pool, endpoint_id).await;

        let response = service
            .read(&provider_endpoint, true)
            .await
            .expect("failed refresh still serves last-good");
        assert_eq!(fetcher.calls_for(endpoint_id), 1);
        assert_eq!(
            db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            db::quota_key_observations::latest_observation(
                &pool,
                endpoint_id,
                response.keys[0].key_id
            )
            .await
            .unwrap()
            .is_none()
        );
        cleanup_endpoint(&pool, endpoint_id).await;
    }
}
