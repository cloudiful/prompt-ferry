use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use tokio::time::{Instant, timeout};
use uuid::Uuid;

use super::{
    ChatGptQuotaError, ChatGptQuotaService, RefreshCompletion,
    policy::{REPLICA_POLL, RefreshTrigger},
    read::{completed_after, completion},
    refresh::RefreshAttempt,
};

const WAIT_FOR_REPLICA: Duration = Duration::from_secs(20);
const SERVICE_TIMEOUT: Duration = Duration::from_secs(43);

#[derive(Default)]
pub(super) struct ReplicaWaitDeadline(Option<Instant>);

impl ReplicaWaitDeadline {
    pub(super) fn get_or_start(&mut self, timeout: Duration) -> Instant {
        *self.0.get_or_insert_with(|| Instant::now() + timeout)
    }
}

pub(super) enum ReplicaWait {
    Completed(RefreshCompletion),
    Retry,
    InProgress(RefreshCompletion),
}

impl ChatGptQuotaService {
    pub(super) async fn refresh_shared(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        periodic_active: Option<bool>,
    ) -> RefreshCompletion {
        self.refresh_with_force(endpoint_id, trigger, arrival, periodic_active, false)
            .await
    }

    pub(super) async fn refresh_shared_forced(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        periodic_active: Option<bool>,
    ) -> RefreshCompletion {
        self.refresh_with_force(endpoint_id, trigger, arrival, periodic_active, true)
            .await
    }

    async fn refresh_with_force(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        periodic_active: Option<bool>,
        force: bool,
    ) -> RefreshCompletion {
        let mut replica_deadline = ReplicaWaitDeadline::default();
        let cycle = run_bounded_cycle(
            SERVICE_TIMEOUT,
            || {
                Box::pin(self.refresh_serialized(
                    endpoint_id,
                    trigger,
                    arrival,
                    periodic_active,
                    force,
                ))
            },
            || {
                let deadline = replica_deadline.get_or_start(WAIT_FOR_REPLICA);
                Box::pin(self.wait_for_replica(endpoint_id, trigger, arrival, deadline))
            },
        );
        match cycle.await {
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

    async fn wait_for_replica(
        &self,
        endpoint_id: Uuid,
        trigger: RefreshTrigger,
        arrival: DateTime<Utc>,
        deadline: Instant,
    ) -> ReplicaWait {
        let mut inactive_lease_seen = false;
        loop {
            let (snapshot, state) = match self.load(endpoint_id).await {
                Ok(value) => value,
                Err(error) => return ReplicaWait::Completed(completion(None, None, Some(error))),
            };
            if completed_after(trigger, arrival, snapshot.as_ref(), state.as_ref()) {
                return ReplicaWait::Completed(completion(snapshot, state, None));
            }
            let lease_active = state.as_ref().is_some_and(|state| {
                state.lease_owner.is_some()
                    && state
                        .lease_expires_at
                        .is_some_and(|expires_at| expires_at > Utc::now())
            });
            let now = Instant::now();
            if now >= deadline {
                return ReplicaWait::InProgress(completion(
                    snapshot,
                    state,
                    Some(ChatGptQuotaError::InProgress),
                ));
            }
            if !lease_active {
                if inactive_lease_seen {
                    return ReplicaWait::Retry;
                }
                inactive_lease_seen = true;
            } else {
                inactive_lease_seen = false;
            }
            tokio::time::sleep(REPLICA_POLL.min(deadline.saturating_duration_since(now))).await;
        }
    }
}

pub(super) async fn run_bounded_cycle<'a>(
    service_timeout: Duration,
    attempt: impl FnMut() -> BoxFuture<'a, RefreshAttempt>,
    wait: impl FnMut() -> BoxFuture<'a, ReplicaWait>,
) -> Result<RefreshCompletion, tokio::time::error::Elapsed> {
    timeout(service_timeout, run_cycle(attempt, wait)).await
}

pub(super) async fn run_cycle<'a>(
    mut attempt: impl FnMut() -> BoxFuture<'a, RefreshAttempt>,
    mut wait: impl FnMut() -> BoxFuture<'a, ReplicaWait>,
) -> RefreshCompletion {
    loop {
        match attempt().await {
            RefreshAttempt::Completed(completion) => return completion,
            RefreshAttempt::LeaseLost => match wait().await {
                ReplicaWait::Completed(completion) | ReplicaWait::InProgress(completion) => {
                    return completion;
                }
                ReplicaWait::Retry => {}
            },
        }
    }
}
