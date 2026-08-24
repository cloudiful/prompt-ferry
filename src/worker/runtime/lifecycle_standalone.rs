//! Standalone request lease lifecycle.
//!
//! This sibling module keeps the standalone acquire/heartbeat/drop/release
//! loop and the stale lease reconciler separate from the existing
//! PostgreSQL/Valkey heartbeat logic in `lifecycle.rs`. The standalone path
//! only writes to `standalone_request_leases`; it must never touch
//! PostgreSQL or Valkey request-record state.
//!
//! The reconcile task only deletes expired lease rows because standalone
//! request records do not exist yet; it must not be presented as aborting
//! a durable request.

use std::time::Duration;

use tokio::{task::JoinHandle, time, time::MissedTickBehavior};
use tracing::warn;
use uuid::Uuid;

use crate::standalone_config::{RequestLeaseAcquireOutcome, StandaloneRequestLeaseStore};
use crate::worker::runtime::lifecycle::RuntimeControl;
use crate::worker::runtime::standalone::StandaloneRuntimeState;

/// Bundle the runtime inputs the lease guard needs so the dispatcher in
/// `runtime/mod.rs` does not have to walk the standalone state boundary.
#[derive(Clone)]
pub(super) struct StandaloneLeaseInputs {
    store: StandaloneRequestLeaseStore,
    worker_instance_id: Uuid,
    lease_seconds: i64,
    heartbeat_seconds: i64,
}

impl StandaloneLeaseInputs {
    pub(super) fn from_standalone_state(
        state: &StandaloneRuntimeState,
        worker_instance_id: Uuid,
        lease_seconds: i64,
        heartbeat_seconds: i64,
    ) -> Self {
        // The standalone runtime exposes the configuration store as a
        // public(super) field; cloning the Arc here avoids taking the
        // full snapshot just to access the pool.
        let pool = state.store_pool();
        Self {
            store: StandaloneRequestLeaseStore::new(pool),
            worker_instance_id,
            lease_seconds,
            heartbeat_seconds,
        }
    }
}

/// Acquire a standalone request lease and spawn the heartbeat loop. Returns
/// `None` when the runtime caller has no standalone lease inputs (e.g. on
/// the PostgreSQL/Valkey path) so the dispatcher in `runtime/mod.rs` can
/// fall back to the existing `RequestLeaseGuard`.
pub(super) fn spawn_standalone_request_lease_guard(
    inputs: Option<StandaloneLeaseInputs>,
    control: RuntimeControl,
    request_id: Uuid,
) -> Option<StandaloneRequestLeaseGuard> {
    let inputs = inputs?;
    let store = inputs.store.clone();
    let handle = tokio::spawn(async move {
        match store
            .acquire(
                request_id,
                inputs.worker_instance_id,
                inputs.lease_seconds.max(1) as u64,
            )
            .await
        {
            Ok(RequestLeaseAcquireOutcome::Acquired) => {}
            Ok(RequestLeaseAcquireOutcome::Blocked) => {
                warn!(
                    %request_id,
                    "standalone request lease held by another worker; guard will not heartbeat"
                );
                return;
            }
            Err(error) => {
                warn!(
                    error = %error,
                    %request_id,
                    "failed to acquire standalone request lease"
                );
                return;
            }
        }

        let mut interval =
            time::interval(Duration::from_secs(inputs.heartbeat_seconds.max(1) as u64));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;

        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = interval.tick() => {
                    if control.is_shutting_down() {
                        break;
                    }
                    match store
                        .refresh(
                            request_id,
                            inputs.worker_instance_id,
                            inputs.lease_seconds.max(1) as u64,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            warn!(
                                %request_id,
                                "stopped standalone request lease heartbeat because the lease is no longer owned"
                            );
                            break;
                        }
                        Err(error) => {
                            warn!(
                                error = %error,
                                %request_id,
                                "failed to refresh standalone request lease"
                            );
                        }
                    }
                }
            }
        }
    });
    Some(StandaloneRequestLeaseGuard {
        handle: Some(handle),
        request_id,
        inputs: Some(inputs),
    })
}

/// Drop handle for a standalone request lease. Aborts the heartbeat task
/// and best-effort releases the owner-checked SQLite row.
pub(super) struct StandaloneRequestLeaseGuard {
    handle: Option<JoinHandle<()>>,
    request_id: Uuid,
    inputs: Option<StandaloneLeaseInputs>,
}

impl Drop for StandaloneRequestLeaseGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        let Some(inputs) = self.inputs.take() else {
            return;
        };
        let request_id = self.request_id;
        let store = inputs.store.clone();
        let owner = inputs.worker_instance_id;
        tokio::spawn(async move {
            if let Err(error) = store.release(request_id, owner).await {
                warn!(
                    error = %error,
                    %request_id,
                    "failed to release standalone request lease"
                );
            }
        });
    }
}

/// Sweep standalone request leases whose `lease_expires_at` has passed.
/// The reconciler only deletes expired lease rows; standalone request
/// records do not exist yet, so this must never be presented as aborting
/// a durable request. Returns `None` when no standalone state is
/// available.
pub(super) fn spawn_standalone_stale_lease_reconciler(
    state: Option<StandaloneRuntimeState>,
    control: RuntimeControl,
    sweep_interval: Duration,
) -> Option<JoinHandle<()>> {
    let state = state?;
    let store = StandaloneRequestLeaseStore::new(state.store_pool());
    let sweep_interval = if sweep_interval.is_zero() {
        Duration::from_secs(1)
    } else {
        sweep_interval
    };
    Some(tokio::spawn(async move {
        // Mirror the PostgreSQL reconciler delay before the first sweep
        // so a fresh restart does not immediately delete expired leases
        // owned by an earlier process before they have a chance to be
        // refreshed through the new heartbeat task.
        tokio::select! {
            _ = control.wait_for_shutdown() => return,
            _ = time::sleep(sweep_interval) => {}
        }
        let mut interval = time::interval(sweep_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = interval.tick() => {
                    match store.abort_stale().await {
                        Ok(count) if count > 0 => {
                            let remaining = store.list_active().await.map(|rows| rows.len()).unwrap_or(0);
                            warn!(
                                count,
                                remaining,
                                "pruned expired standalone request lease rows"
                            );
                        }
                        Ok(_) => {}
                        Err(error) => {
                            warn!(
                                error = %error,
                                "failed to prune expired standalone request lease rows"
                            );
                        }
                    }
                }
            }
        }
    }))
}
