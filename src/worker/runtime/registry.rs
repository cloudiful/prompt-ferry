//! Worker registry lifecycle: admit the process before it serves, keep the
//! registry row current while it is live, and mark the process stopped on
//! shutdown.
//!
//! Admission is a gate: the identity is claimed before any service starts,
//! and a collision is rejected immediately instead of being retried while the
//! worker serves. A worker that loses its fencing stops serving instead of
//! continuing with a dead identity.
//!
//! The lease is bounded from the admission instant: the heartbeat task and
//! the watchdog both measure the remaining lease against the last successful
//! heartbeat, so a stalled or failing database stops the worker before
//! another process can claim the identity.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use super::lifecycle::RuntimeControl;
use crate::config::WorkerConfig;
use crate::db;

/// How long a failed heartbeat may be retried while the registry row is
/// still proven to be live.
///
/// The row expires `WORKER_OFFLINE_AFTER_SECONDS` after the last successful
/// heartbeat, so a retry is only allowed while the remaining lease covers it.
const HEARTBEAT_RETRY_GRACE_SECONDS: u64 = 5;

/// Upper bound for one registry database operation.
///
/// A stalled query must not keep the worker alive past its lease, so every
/// registry read and write is bounded.
const REGISTRY_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound for one whole heartbeat attempt.
///
/// A heartbeat reads the desired configuration revision and then writes the
/// heartbeat, so it spends up to two operation deadlines before it is known
/// to have succeeded.
const REGISTRY_HEARTBEAT_BUDGET: Duration = Duration::from_secs(10);

/// How often the lease watchdog checks the remaining lease.
const LEASE_WATCHDOG_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Whether a failed heartbeat may still be retried.
///
/// The registry row expires `lease` after the last successful heartbeat, so a
/// retry is only safe while the remaining lease covers the attempt that is
/// about to start. The budget covers the whole attempt, so a retry that
/// starts inside the remaining lease cannot outlive it.
pub(super) fn may_retry_heartbeat(elapsed_since_success: Duration, lease: Duration) -> bool {
    lease_covers_heartbeat(
        elapsed_since_success,
        lease,
        REGISTRY_HEARTBEAT_BUDGET,
        LEASE_WATCHDOG_CHECK_INTERVAL,
    )
}

/// Whether the remaining lease still covers a heartbeat that is in flight.
///
/// The renewal timestamp is taken before the database write, so the local
/// estimate of the remaining lease is never longer than the lease the
/// database grants. The budget also covers the operation deadline of the
/// heartbeat and the interval of the watchdog, so the worker stops before
/// the recorded lease can expire.
pub(super) fn lease_covers_heartbeat(
    elapsed_since_success: Duration,
    lease: Duration,
    operation_timeout: Duration,
    check_interval: Duration,
) -> bool {
    elapsed_since_success
        + operation_timeout
        + check_interval
        + Duration::from_secs(HEARTBEAT_RETRY_GRACE_SECONDS)
        < lease
}

/// The delay before the next heartbeat attempt.
///
/// A failed heartbeat is retried promptly while the remaining lease still
/// covers it. The retry does not wait for the next heartbeat interval: a
/// failure in the last interval before the expiry would otherwise reach the
/// expiry before the next attempt. `None` means the worker must stop
/// instead of retrying.
pub(super) fn heartbeat_retry_delay(
    elapsed_since_success: Duration,
    lease: Duration,
) -> Option<Duration> {
    if may_retry_heartbeat(elapsed_since_success, lease) {
        Some(Duration::from_secs(1))
    } else {
        None
    }
}

/// The lease state of this incarnation.
///
/// The tracker measures the time since the last successful heartbeat, which
/// starts at the admission instant: the admission itself writes the first
/// heartbeat, so the lease is never renewed from a later task start.
#[derive(Clone)]
pub(super) struct LeaseTracker {
    elapsed_millis: Arc<AtomicU64>,
}

impl LeaseTracker {
    pub(super) fn new() -> Self {
        Self {
            elapsed_millis: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Record a successful heartbeat.
    ///
    /// The instant is the one taken before the database write, so the measured
    /// lease is never longer than the lease the database granted.
    fn record_success(&self, admitted_at: Instant, renewal_instant: Instant) {
        let elapsed = renewal_instant
            .saturating_duration_since(admitted_at)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        self.elapsed_millis.store(elapsed, Ordering::SeqCst);
    }

    /// The time that passed since the last successful heartbeat.
    fn elapsed_since_success(&self, admitted_at: Instant) -> Duration {
        let now = admitted_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let last = self.elapsed_millis.load(Ordering::SeqCst);
        Duration::from_millis(now.saturating_sub(last))
    }
}

/// Admit this process to the worker registry.
///
/// The admission is atomic: the identity and the bootstrap capacity are
/// checked under a database lock, so a second process with the same identity
/// is rejected instead of taking the row over. The rejection is immediate;
/// the operator retries after the previous incarnation expired.
///
/// The returned instant is the admission instant: the registry row expires
/// relative to the heartbeat the admission wrote, so every later lease
/// calculation starts here.
pub(super) async fn admit_worker(
    pool: &sqlx::PgPool,
    config: &WorkerConfig,
    control: &RuntimeControl,
    coordination: db::CoordinationSnapshot,
) -> anyhow::Result<Instant> {
    let worker_id = config.resolved_worker_identity();
    let role = "node";
    let version = env!("CARGO_PKG_VERSION").to_string();
    let incarnation = control.worker_instance_id();
    let restart_required = match db::desired_shared_valkey_revision(pool).await {
        Ok(desired) => desired.is_some_and(|desired| coordination.revision != Some(desired)),
        Err(err) => {
            warn!(error = %err, "failed to read the desired shared configuration revision");
            false
        }
    };
    // The admission instant is taken before the registration writes the first
    // heartbeat, so the local lease is never longer than the database lease.
    let admitted_at = Instant::now();
    let outcome = db::register_worker(
        pool,
        &worker_id,
        role,
        &version,
        incarnation,
        coordination,
        restart_required,
    )
    .await?;
    match outcome {
        db::RegisterOutcome::Registered => {
            info!(%worker_id, "node admitted to the registry");
            Ok(admitted_at)
        }
        db::RegisterOutcome::IdentityCollision => Err(anyhow::anyhow!(
            "another live node owns the identity '{worker_id}'; retry after it expired"
        )),
        db::RegisterOutcome::WorkerLimitReached => Err(anyhow::anyhow!(
            "the degraded bootstrap mode already has a live node; retry after it expired"
        )),
        db::RegisterOutcome::CoordinationIncompatible => Err(anyhow::anyhow!(
            "the worker coordination mode or applied shared configuration revision is incompatible \
             with the live registry; stop the existing workers and start every worker on the \
             current desired revision"
        )),
    }
}

/// Release the registry row of this incarnation.
pub(super) async fn release_worker(pool: &sqlx::PgPool, worker_id: &str, incarnation: uuid::Uuid) {
    match db::stop_worker(pool, worker_id, incarnation).await {
        Ok(true) => info!(%worker_id, "worker registry entry marked stopped"),
        Ok(false) => {}
        Err(err) => warn!(error = %err, "failed to mark the worker registry entry stopped"),
    }
}

/// Outcome of one bounded heartbeat attempt.
enum HeartbeatOutcome {
    /// The registry row was refreshed.
    Renewed,
    /// The registry row belongs to a newer incarnation.
    FencingLost,
}

/// Keep the registry row of the admitted worker current until the worker
/// stops serving.
///
/// The heartbeat keeps running while the worker drains its accepted work: the
/// row is released by the caller only after every service and every in-flight
/// execution stopped, so a replacement worker never serves next to a worker
/// that still executes accepted requests.
pub(super) fn spawn_worker_registry(
    pool: sqlx::PgPool,
    config: &WorkerConfig,
    control: &RuntimeControl,
    coordination: db::CoordinationSnapshot,
    admitted_at: Instant,
    tracker: LeaseTracker,
    stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let worker_id = config.resolved_worker_identity();
    let incarnation = control.worker_instance_id();
    let config_applied_revision = coordination.revision;
    let coordination_mode = coordination.mode;
    let control = control.clone();
    tokio::spawn(async move {
        let lease = Duration::from_secs(db::WORKER_OFFLINE_AFTER_SECONDS as u64);
        let heartbeat_interval = Duration::from_secs(db::WORKER_HEARTBEAT_SECONDS);
        let mut next_heartbeat = admitted_at + heartbeat_interval;
        loop {
            let wait = tokio::time::sleep_until(tokio::time::Instant::from_std(next_heartbeat));
            tokio::pin!(wait);
            tokio::select! {
                _ = wait_for_registry_stop(stop.clone()) => break,
                _ = &mut wait => {}
            }

            // The renewal timestamp is taken before the database write: the
            // database sets its heartbeat timestamp when it executes the
            // statement, so the local estimate of the remaining lease must
            // never be longer than the lease the database granted.
            let renewal_instant = Instant::now();
            match heartbeat_once(
                &pool,
                &worker_id,
                incarnation,
                config_applied_revision,
                coordination_mode,
            )
            .await
            {
                Ok(HeartbeatOutcome::Renewed) => {
                    tracker.record_success(admitted_at, renewal_instant);
                    next_heartbeat = Instant::now() + heartbeat_interval;
                }
                Ok(HeartbeatOutcome::FencingLost) => {
                    // The registry row belongs to a newer incarnation:
                    // this process lost its fencing and must not keep
                    // serving requests under a dead identity.
                    warn!(
                        %worker_id,
                        "worker registry entry belongs to a newer incarnation; stopping the worker service"
                    );
                    control.begin_fail_closed_shutdown();
                    break;
                }
                Err(err) => {
                    // The heartbeat failed. The row expires after the offline
                    // window, so the worker may only retry while the remaining
                    // lease still covers it.
                    let elapsed = tracker.elapsed_since_success(admitted_at);
                    match heartbeat_retry_delay(elapsed, lease) {
                        Some(retry_delay) => {
                            warn!(
                                error = %err,
                                remaining_lease_seconds = lease.saturating_sub(elapsed).as_secs(),
                                "worker registry heartbeat failed; retrying inside the remaining lease"
                            );
                            next_heartbeat = Instant::now() + retry_delay;
                        }
                        None => {
                            warn!(
                                error = %err,
                                "worker registry heartbeat failed and the registry lease is about to expire; \
                                 stopping the worker service"
                            );
                            control.begin_fail_closed_shutdown();
                            break;
                        }
                    }
                }
            }
        }
    })
}

/// Stop the worker service when the registry lease is about to expire.
///
/// The watchdog runs next to the heartbeat task, so a heartbeat that hangs
/// inside the database cannot keep the worker alive past its lease. It stops
/// the worker conservatively: the identity may already be claimed by another
/// process, so the work is cancelled instead of drained.
pub(super) fn spawn_lease_watchdog(
    control: &RuntimeControl,
    admitted_at: Instant,
    tracker: LeaseTracker,
    stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let control = control.clone();
    tokio::spawn(async move {
        let lease = Duration::from_secs(db::WORKER_OFFLINE_AFTER_SECONDS as u64);
        loop {
            tokio::select! {
                _ = wait_for_registry_stop(stop.clone()) => break,
                _ = tokio::time::sleep(LEASE_WATCHDOG_CHECK_INTERVAL) => {}
            }
            let elapsed = tracker.elapsed_since_success(admitted_at);
            if !lease_covers_heartbeat(
                elapsed,
                lease,
                REGISTRY_HEARTBEAT_BUDGET,
                LEASE_WATCHDOG_CHECK_INTERVAL,
            ) {
                warn!(
                    remaining_lease_seconds = lease.saturating_sub(elapsed).as_secs(),
                    "the worker registry lease is about to expire; stopping the worker service"
                );
                control.begin_fail_closed_shutdown();
                break;
            }
        }
    })
}

/// Resolve when the worker asked the registry tasks to stop.
async fn wait_for_registry_stop(mut stop: watch::Receiver<bool>) {
    if *stop.borrow() {
        return;
    }
    let _ = stop.changed().await;
}

/// Run one bounded heartbeat attempt.
///
/// The desired revision read and the heartbeat write are both bounded: a
/// stalled query is a failed attempt, not a reason to keep serving.
async fn heartbeat_once(
    pool: &sqlx::PgPool,
    worker_id: &str,
    incarnation: uuid::Uuid,
    config_applied_revision: Option<i64>,
    coordination_mode: db::WorkerCoordinationMode,
) -> anyhow::Result<HeartbeatOutcome> {
    let desired = tokio::time::timeout(
        REGISTRY_OPERATION_TIMEOUT,
        db::desired_shared_valkey_revision(pool),
    )
    .await
    .map_err(|_| anyhow::anyhow!("the desired shared configuration revision read timed out"))??;
    let restart_required = desired.is_some_and(|desired| config_applied_revision != Some(desired));
    let renewed = tokio::time::timeout(
        REGISTRY_OPERATION_TIMEOUT,
        db::heartbeat_worker(
            pool,
            worker_id,
            incarnation,
            config_applied_revision,
            restart_required,
            coordination_mode,
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("the worker registry heartbeat timed out"))??;
    Ok(if renewed {
        HeartbeatOutcome::Renewed
    } else {
        HeartbeatOutcome::FencingLost
    })
}

#[cfg(test)]
mod tests {
    use super::{
        HEARTBEAT_RETRY_GRACE_SECONDS, LeaseTracker, heartbeat_retry_delay, lease_covers_heartbeat,
        may_retry_heartbeat,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn a_failed_heartbeat_is_only_retried_inside_the_remaining_lease() {
        let lease = Duration::from_secs(45);
        assert!(may_retry_heartbeat(Duration::from_secs(1), lease));
        // The retry budget covers the whole heartbeat attempt, so the last
        // safe moment is well before the expiry.
        assert!(!may_retry_heartbeat(
            lease - Duration::from_secs(HEARTBEAT_RETRY_GRACE_SECONDS),
            lease
        ));
        assert!(!may_retry_heartbeat(lease, lease));
    }

    #[test]
    fn a_failed_heartbeat_is_retried_promptly_and_stops_before_the_expiry() {
        let lease = Duration::from_secs(45);
        // The retry does not wait for the next heartbeat interval: a failure
        // late in the last interval would otherwise reach the expiry first.
        assert_eq!(
            heartbeat_retry_delay(Duration::from_secs(15), lease),
            Some(Duration::from_secs(1))
        );
        // The last safe moment before the expiry stops the worker.
        assert_eq!(
            heartbeat_retry_delay(
                lease - Duration::from_secs(HEARTBEAT_RETRY_GRACE_SECONDS),
                lease
            ),
            None
        );
        assert_eq!(heartbeat_retry_delay(lease, lease), None);
    }

    #[test]
    fn the_watchdog_budget_covers_the_operation_and_the_check_interval() {
        let lease = Duration::from_secs(45);
        let operation = Duration::from_secs(5);
        let check = Duration::from_secs(1);
        // The lease still covers a heartbeat that is in flight.
        assert!(lease_covers_heartbeat(
            Duration::from_secs(30),
            lease,
            operation,
            check
        ));
        // The last moment that still covers it stops the worker, before the
        // recorded lease can expire.
        assert!(!lease_covers_heartbeat(
            lease - operation - check - Duration::from_secs(HEARTBEAT_RETRY_GRACE_SECONDS),
            lease,
            operation,
            check
        ));
        assert!(!lease_covers_heartbeat(lease, lease, operation, check));
    }

    #[test]
    fn the_lease_is_measured_from_the_admission_and_not_from_a_later_start() {
        let admitted_at = Instant::now();
        let tracker = LeaseTracker::new();
        // The admission itself is the first successful heartbeat.
        assert!(tracker.elapsed_since_success(admitted_at) < Duration::from_secs(1));

        std::thread::sleep(Duration::from_millis(20));
        tracker.record_success(admitted_at, Instant::now());
        let after_success = tracker.elapsed_since_success(admitted_at);
        assert!(after_success < Duration::from_secs(1));
    }
}
