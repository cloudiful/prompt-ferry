use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex as StdMutex, Weak},
    time::{Duration, Instant},
};

use chrono::Utc;
use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use tokio::sync::{Mutex, Semaphore, mpsc};
use uuid::Uuid;

use crate::db::{self, ChatgptQuotaRefreshState, ChatgptQuotaSnapshot, ConfigRepository};

mod fetch;
mod policy;
mod read;
mod refresh;
#[cfg(test)]
pub(crate) mod tests;

pub(crate) use fetch::ChatGptQuotaFetcher;
use fetch::StoredOAuthQuotaFetcher;
use policy::{ACTIVITY_LOOKBACK, RefreshTrigger, periodic_due, stale};

const REQUEST_WAKE_QUEUE_CAPACITY: usize = 256;
const REQUEST_WAKE_THROTTLE: Duration = Duration::from_secs(60);
pub(crate) const MAX_CONCURRENT_QUOTA_READS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatGptQuotaError {
    NotConfigured,
    Auth,
    Upstream,
    Timeout,
    InvalidQuota,
    Storage,
    InProgress,
}

impl ChatGptQuotaError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Auth => "auth",
            Self::Upstream => "upstream",
            Self::Timeout => "timeout",
            Self::InvalidQuota => "invalid_quota",
            Self::Storage => "storage",
            Self::InProgress => "in_progress",
        }
    }
}

impl std::fmt::Display for ChatGptQuotaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for ChatGptQuotaError {}

#[derive(Clone)]
pub(super) struct RefreshCompletion {
    pub snapshot: Option<ChatgptQuotaSnapshot>,
    pub state: Option<ChatgptQuotaRefreshState>,
    pub error: Option<ChatGptQuotaError>,
}

pub(crate) struct ChatGptQuotaService {
    pub(super) inner: Arc<ServiceInner>,
}

pub(super) struct ServiceInner {
    pub pool: PgPool,
    pub repository: ConfigRepository,
    pub fetcher: Arc<dyn ChatGptQuotaFetcher>,
    pub endpoint_locks: Mutex<HashMap<Uuid, Weak<Mutex<()>>>>,
    pub quota_slots: Arc<Semaphore>,
    request_wake_tx: mpsc::Sender<Uuid>,
    request_wake_rx: StdMutex<Option<mpsc::Receiver<Uuid>>>,
    last_request_wake: StdMutex<HashMap<Uuid, Instant>>,
}

impl ChatGptQuotaService {
    pub(crate) fn new(pool: PgPool, repository: ConfigRepository) -> Self {
        Self::with_fetcher(pool, repository, Arc::new(StoredOAuthQuotaFetcher))
    }

    pub(crate) fn with_fetcher(
        pool: PgPool,
        repository: ConfigRepository,
        fetcher: Arc<dyn ChatGptQuotaFetcher>,
    ) -> Self {
        let (request_wake_tx, request_wake_rx) = mpsc::channel(REQUEST_WAKE_QUEUE_CAPACITY);
        Self {
            inner: Arc::new(ServiceInner {
                pool,
                repository,
                fetcher,
                endpoint_locks: Mutex::new(HashMap::new()),
                quota_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_QUOTA_READS)),
                request_wake_tx,
                request_wake_rx: StdMutex::new(Some(request_wake_rx)),
                last_request_wake: StdMutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn take_request_wake_receiver(&self) -> Option<mpsc::Receiver<Uuid>> {
        self.inner
            .request_wake_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    pub(crate) fn notify_selected_request(&self, endpoint_id: Uuid) {
        if endpoint_id.is_nil() {
            return;
        }
        let now = Instant::now();
        let mut last_wake = self
            .inner
            .last_request_wake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        last_wake.retain(|_, seen| now.duration_since(*seen) < REQUEST_WAKE_THROTTLE);
        if last_wake
            .get(&endpoint_id)
            .is_some_and(|seen| now.duration_since(*seen) < REQUEST_WAKE_THROTTLE)
        {
            return;
        }
        last_wake.insert(endpoint_id, now);
        if self.inner.request_wake_tx.try_send(endpoint_id).is_err() {
            last_wake.remove(&endpoint_id);
        }
    }

    pub(crate) async fn refresh_selected_request(
        &self,
        endpoint_id: Uuid,
    ) -> Result<(), ChatGptQuotaError> {
        let now = Utc::now();
        let (snapshot, state) = self.load(endpoint_id).await?;
        if snapshot
            .as_ref()
            .is_some_and(|snapshot| !stale(snapshot.observed_at, now))
        {
            return Ok(());
        }
        if state
            .as_ref()
            .and_then(|state| state.next_retry_at)
            .is_some_and(|retry_at| retry_at > now)
        {
            return Ok(());
        }
        let completion = self
            .refresh_shared(endpoint_id, RefreshTrigger::Request, now, None)
            .await;
        if completion.snapshot.is_none() {
            return Err(completion.error.unwrap_or(ChatGptQuotaError::Upstream));
        }
        Ok(())
    }

    pub(crate) async fn run_periodic_pass(&self) -> Result<(), ChatGptQuotaError> {
        let now = Utc::now();
        let endpoint_ids = db::quota_snapshots::eligible_endpoints(&self.inner.pool)
            .await
            .map_err(|_| ChatGptQuotaError::Storage)?;
        let activity = db::quota_snapshots::endpoint_activity(
            &self.inner.pool,
            &endpoint_ids,
            now - ACTIVITY_LOOKBACK,
        )
        .await
        .map_err(|_| ChatGptQuotaError::Storage)?;
        let active = activity
            .into_iter()
            .map(|activity| activity.endpoint_id)
            .collect::<HashSet<_>>();
        stream::iter(endpoint_ids)
            .for_each_concurrent(MAX_CONCURRENT_QUOTA_READS, |endpoint_id| {
                let active = active.contains(&endpoint_id);
                async move {
                    if let Err(error) = self
                        .refresh_periodic_endpoint(endpoint_id, active, now)
                        .await
                    {
                        tracing::warn!(
                            endpoint_id = %endpoint_id,
                            error = error.code(),
                            "ChatGPT quota collector refresh failed"
                        );
                    }
                }
            })
            .await;
        db::quota_snapshots::prune_expired_snapshots(
            &self.inner.pool,
            now,
            db::quota_snapshots::SNAPSHOT_PRUNE_BATCH_SIZE,
        )
        .await
        .map_err(|_| ChatGptQuotaError::Storage)?;
        Ok(())
    }

    async fn refresh_periodic_endpoint(
        &self,
        endpoint_id: Uuid,
        active: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<(), ChatGptQuotaError> {
        let (_, state) = self.load(endpoint_id).await?;
        if !periodic_due(
            state.as_ref().and_then(|state| state.last_success_at),
            active,
            now,
        ) || state
            .as_ref()
            .and_then(|state| state.next_retry_at)
            .is_some_and(|retry_at| retry_at > now)
        {
            return Ok(());
        }
        let completion = self
            .refresh_shared(endpoint_id, RefreshTrigger::Periodic, now, Some(active))
            .await;
        if completion.snapshot.is_none() {
            return Err(completion.error.unwrap_or(ChatGptQuotaError::Upstream));
        }
        Ok(())
    }
}
