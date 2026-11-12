use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::future::BoxFuture;
use tokio::time::{sleep, sleep_until, timeout};

use super::super::{
    ChatGptQuotaError, RefreshCompletion,
    refresh::RefreshAttempt,
    refresh_cycle::{ReplicaWait, ReplicaWaitDeadline, run_bounded_cycle, run_cycle},
};

fn empty_completion() -> RefreshCompletion {
    RefreshCompletion {
        snapshot: None,
        state: None,
        error: None,
    }
}

#[tokio::test]
async fn iterative_cycle_handles_many_lease_losses() {
    const LEASE_LOSSES: usize = 64;

    let attempts = Arc::new(AtomicUsize::new(0));
    let waits = Arc::new(AtomicUsize::new(0));
    let completion = run_cycle(
        {
            let attempts = attempts.clone();
            move || {
                let attempt = if attempts.fetch_add(1, Ordering::SeqCst) < LEASE_LOSSES {
                    RefreshAttempt::LeaseLost
                } else {
                    RefreshAttempt::Completed(Box::new(empty_completion()))
                };
                Box::pin(async move { attempt }) as BoxFuture<'static, _>
            }
        },
        {
            let waits = waits.clone();
            move || {
                waits.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { ReplicaWait::Retry }) as BoxFuture<'static, _>
            }
        },
    )
    .await;

    assert_eq!(attempts.load(Ordering::SeqCst), LEASE_LOSSES + 1);
    assert_eq!(waits.load(Ordering::SeqCst), LEASE_LOSSES);
    assert_eq!(completion.error, None);
}

#[tokio::test]
async fn replica_deadline_is_reused_and_returns_in_progress() {
    let mut budget = ReplicaWaitDeadline::default();
    let observed_deadlines = Arc::new(Mutex::new(Vec::new()));
    let waits = Arc::new(AtomicUsize::new(0));
    let completion = timeout(
        Duration::from_millis(150),
        run_cycle(
            || Box::pin(async { RefreshAttempt::LeaseLost }) as BoxFuture<'static, _>,
            {
                let observed_deadlines = observed_deadlines.clone();
                let waits = waits.clone();
                move || {
                    let deadline = budget.get_or_start(Duration::from_millis(35));
                    observed_deadlines.lock().unwrap().push(deadline);
                    let wait = waits.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move {
                        if wait == 0 {
                            sleep(Duration::from_millis(5)).await;
                            ReplicaWait::Retry
                        } else {
                            sleep_until(deadline).await;
                            ReplicaWait::InProgress(RefreshCompletion {
                                snapshot: None,
                                state: None,
                                error: Some(ChatGptQuotaError::InProgress),
                            })
                        }
                    }) as BoxFuture<'static, _>
                }
            },
        ),
    )
    .await
    .expect("replica waiting remains bounded");

    let deadlines = observed_deadlines.lock().unwrap();
    assert_eq!(deadlines.len(), 2);
    assert_eq!(deadlines[0], deadlines[1]);
    assert_eq!(waits.load(Ordering::SeqCst), 2);
    assert_eq!(completion.error, Some(ChatGptQuotaError::InProgress));
}

#[tokio::test]
async fn service_deadline_bounds_the_entire_retry_cycle() {
    let waits = Arc::new(AtomicUsize::new(0));
    let result = timeout(
        Duration::from_millis(150),
        run_bounded_cycle(
            Duration::from_millis(30),
            || Box::pin(async { RefreshAttempt::LeaseLost }) as BoxFuture<'static, _>,
            {
                let waits = waits.clone();
                move || {
                    waits.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async {
                        sleep(Duration::from_millis(5)).await;
                        ReplicaWait::Retry
                    }) as BoxFuture<'static, _>
                }
            },
        ),
    )
    .await
    .expect("the shared cycle timeout completes");

    assert!(result.is_err());
    assert!(waits.load(Ordering::SeqCst) >= 2);
}
