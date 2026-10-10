use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{db::ConfigRepository, worker_admin::handlers::chatgpt_backend::ChatgptQuota};

use super::{ChatGptQuotaError, ChatGptQuotaFetcher};

mod concurrency;
mod failure;
mod policy;
mod support;

pub(crate) fn fake_fetcher(quota: ChatgptQuota) -> FakeFetcher {
    FakeFetcher::success(quota)
}

pub(crate) struct FakeFetcher {
    pub calls: Arc<std::sync::atomic::AtomicUsize>,
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
            result,
            started: None,
            release: None,
        }
    }
}

impl ChatGptQuotaFetcher for FakeFetcher {
    fn fetch<'a>(
        &'a self,
        _endpoint_id: Uuid,
        _repository: &'a ConfigRepository,
    ) -> BoxFuture<'a, Result<ChatgptQuota, ChatGptQuotaError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
