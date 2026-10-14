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
