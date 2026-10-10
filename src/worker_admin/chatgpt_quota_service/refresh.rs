use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    db::{ChatgptQuotaSnapshotCreate, quota_snapshots},
    worker_admin::chatgpt_quota_normalize,
};

use super::{
    ChatGptQuotaError, ChatGptQuotaService, RefreshCompletion,
    policy::{FAILURE_BACKOFF_BASE_SECONDS, RefreshTrigger, periodic_due, retry_delay, stale},
    read::{completed_after, completion, error_from_state},
};

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const SERVICE_TIMEOUT: Duration = Duration::from_secs(43);

impl ChatGptQuotaService {
    pub(super) async fn refresh_shared(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        periodic_active: Option<bool>,
    ) -> RefreshCompletion {
        match timeout(
            SERVICE_TIMEOUT,
            self.refresh_serialized(endpoint_id, trigger, arrival, periodic_active),
        )
        .await
        {
            Ok(completion) => completion,
            Err(_) => {
                tracing::warn!(endpoint_id = %endpoint_id, "ChatGPT quota refresh exceeded its bounded service time");
                match timeout(Duration::from_secs(1), self.load(endpoint_id)).await {
                    Ok(Ok((snapshot, state))) => {
                        completion(snapshot, state, Some(ChatGptQuotaError::InProgress))
                    }
                    Ok(Err(error)) => completion(None, None, Some(error)),
                    Err(_) => completion(None, None, Some(ChatGptQuotaError::InProgress)),
                }
            }
        }
    }

    async fn refresh_serialized(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        periodic_active: Option<bool>,
    ) -> RefreshCompletion {
        let endpoint_lock = {
            let mut locks = self.inner.endpoint_locks.lock().await;
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(&endpoint_id).and_then(std::sync::Weak::upgrade) {
                lock
            } else {
                let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(endpoint_id, std::sync::Arc::downgrade(&lock));
                lock
            }
        };
        let endpoint_guard = endpoint_lock.lock().await;
        let (snapshot, state) = match self.load(endpoint_id).await {
            Ok(value) => value,
            Err(error) => return completion(None, None, Some(error)),
        };
        if completed_after(trigger, arrival, snapshot.as_ref(), state.as_ref()) {
            return completion(snapshot, state, None);
        }
        if !trigger.is_manual() {
            let now = Utc::now();
            let due = match trigger {
                RefreshTrigger::Request => snapshot
                    .as_ref()
                    .is_none_or(|snapshot| stale(snapshot.observed_at, now)),
                RefreshTrigger::Periodic => periodic_due(
                    state.as_ref().and_then(|state| state.last_success_at),
                    periodic_active.unwrap_or(false),
                    now,
                ),
                RefreshTrigger::Manual => true,
            };
            if !due
                || state
                    .as_ref()
                    .and_then(|state| state.next_retry_at)
                    .is_some_and(|retry_at| retry_at > now)
            {
                return completion(
                    snapshot,
                    state.clone(),
                    state.as_ref().and_then(error_from_state),
                );
            }
        }
        let permit = match self.inner.quota_slots.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return completion(snapshot, state, Some(ChatGptQuotaError::InProgress)),
        };
        let (shared_snapshot, shared_state) = match self.load(endpoint_id).await {
            Ok(value) => value,
            Err(error) => return completion(snapshot, None, Some(error)),
        };
        if completed_after(
            trigger,
            arrival,
            shared_snapshot.as_ref(),
            shared_state.as_ref(),
        ) {
            return completion(shared_snapshot, shared_state, None);
        }
        let (snapshot, state) = (shared_snapshot, shared_state);
        let owner = Uuid::new_v4();
        let attempted_at = Utc::now();
        let lease_expires_at =
            attempted_at + chrono::Duration::seconds(quota_snapshots::REFRESH_LEASE_MAX_SECONDS);
        let acquired = match quota_snapshots::acquire_refresh_lease(
            &self.inner.pool,
            endpoint_id,
            owner,
            attempted_at,
            lease_expires_at,
        )
        .await
        {
            Ok(acquired) => acquired,
            Err(_) => return completion(snapshot, state, Some(ChatGptQuotaError::Storage)),
        };
        if !acquired {
            drop(permit);
            drop(endpoint_guard);
            return self
                .wait_for_replica(endpoint_id, trigger, arrival, periodic_active)
                .await;
        }
        let previous_failures = state
            .as_ref()
            .map(|state| state.consecutive_failures)
            .unwrap_or_default();
        let fetched = timeout(
            FETCH_TIMEOUT,
            self.inner
                .fetcher
                .fetch(endpoint_id, &self.inner.repository),
        )
        .await
        .unwrap_or(Err(ChatGptQuotaError::Timeout));
        let observed_at = Utc::now();
        match fetched {
            Ok(quota) => {
                let windows = match serde_json::to_value(
                    chatgpt_quota_normalize::subscription_windows(&quota),
                ) {
                    Ok(windows) => windows,
                    Err(_) => {
                        return self
                            .finish_failure(
                                endpoint_id,
                                owner,
                                previous_failures,
                                ChatGptQuotaError::InvalidQuota,
                            )
                            .await;
                    }
                };
                let snapshot = ChatgptQuotaSnapshotCreate {
                    endpoint_id,
                    observed_at,
                    plan_type: quota.plan_type,
                    limit_reached: quota.limit_reached,
                    windows,
                    source: trigger.source(),
                };
                match quota_snapshots::complete_refresh_success(&self.inner.pool, owner, snapshot)
                    .await
                {
                    Ok(true) => match self.load(endpoint_id).await {
                        Ok((snapshot, state)) => completion(snapshot, state, None),
                        Err(error) => completion(None, None, Some(error)),
                    },
                    Ok(false) => {
                        let (snapshot, state) =
                            self.load(endpoint_id).await.unwrap_or((None, None));
                        completion(snapshot, state, Some(ChatGptQuotaError::InProgress))
                    }
                    Err(_) => {
                        self.finish_failure(
                            endpoint_id,
                            owner,
                            previous_failures,
                            ChatGptQuotaError::Storage,
                        )
                        .await
                    }
                }
            }
            Err(error) => {
                self.finish_failure(endpoint_id, owner, previous_failures, error)
                    .await
            }
        }
    }

    async fn finish_failure(
        &self,
        endpoint_id: Uuid,
        owner: Uuid,
        previous_failures: i32,
        error: ChatGptQuotaError,
    ) -> RefreshCompletion {
        let completed_at = Utc::now();
        let failures = previous_failures.saturating_add(1);
        let delay =
            retry_delay(failures).max(chrono::Duration::seconds(FAILURE_BACKOFF_BASE_SECONDS));
        let next_retry_at = completed_at + delay;
        match quota_snapshots::complete_refresh_failure(
            &self.inner.pool,
            endpoint_id,
            owner,
            completed_at,
            Some(next_retry_at),
            Some(error.code()),
        )
        .await
        {
            Ok(true) => match self.load(endpoint_id).await {
                Ok((snapshot, state)) => completion(snapshot, state, Some(error)),
                Err(_) => completion(None, None, Some(ChatGptQuotaError::Storage)),
            },
            Ok(false) => {
                let (snapshot, state) = self.load(endpoint_id).await.unwrap_or((None, None));
                completion(snapshot, state, Some(ChatGptQuotaError::InProgress))
            }
            Err(_) => {
                let (snapshot, state) = self.load(endpoint_id).await.unwrap_or((None, None));
                completion(snapshot, state, Some(ChatGptQuotaError::Storage))
            }
        }
    }
}
