use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use tokio::{
    sync::Notify,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing::warn;
use uuid::Uuid;

use crate::{db, worker_admin::AdminState};

#[derive(Clone)]
pub(super) struct RuntimeControl {
    worker_instance_id: Uuid,
    shutting_down: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    admission_lock: Arc<Mutex<()>>,
    session_stopped: Arc<AtomicBool>,
    session_stop_notify: Arc<Notify>,
    active_requests: Arc<AtomicUsize>,
    active_notify: Arc<Notify>,
}

impl RuntimeControl {
    pub(super) fn new() -> Self {
        Self {
            worker_instance_id: Uuid::new_v4(),
            shutting_down: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
            admission_lock: Arc::new(Mutex::new(())),
            session_stopped: Arc::new(AtomicBool::new(false)),
            session_stop_notify: Arc::new(Notify::new()),
            active_requests: Arc::new(AtomicUsize::new(0)),
            active_notify: Arc::new(Notify::new()),
        }
    }

    pub(super) fn worker_instance_id(&self) -> Uuid {
        self.worker_instance_id
    }

    pub(super) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    pub(super) fn begin_shutdown(&self) {
        let _admission = self
            .admission_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.shutting_down.swap(true, Ordering::SeqCst) {
            self.shutdown_notify.notify_waiters();
            self.active_notify.notify_waiters();
        }
    }

    pub(super) fn is_session_stopped(&self) -> bool {
        self.session_stopped.load(Ordering::SeqCst)
    }

    /// Close the bridge sessions once the accepted rounds have drained. It is
    /// separate from admission shutdown so a supervisor keeps its existing
    /// session loops alive while those rounds are still running.
    pub(super) fn begin_session_stop(&self) {
        if !self.session_stopped.swap(true, Ordering::SeqCst) {
            self.session_stop_notify.notify_waiters();
        }
    }

    pub(super) async fn wait_for_shutdown(&self) {
        let notified = self.shutdown_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_shutting_down() {
            return;
        }
        notified.await;
    }

    pub(super) async fn wait_for_session_stop(&self) {
        let notified = self.session_stop_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_session_stopped() {
            return;
        }
        notified.await;
    }

    pub(super) fn current_lease(&self) -> (Uuid, DateTime<Utc>, DateTime<Utc>) {
        let last_heartbeat_at = Utc::now();
        let lease_expires_at =
            last_heartbeat_at + chrono::Duration::seconds(super::REQUEST_RECORD_LEASE_SECONDS);
        (self.worker_instance_id, lease_expires_at, last_heartbeat_at)
    }

    pub(super) fn try_track_request(&self) -> Option<ActiveRequestGuard> {
        let _admission = self
            .admission_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.is_shutting_down() {
            return None;
        }
        self.active_requests.fetch_add(1, Ordering::SeqCst);
        Some(ActiveRequestGuard {
            control: self.clone(),
        })
    }

    pub(super) async fn wait_for_drain(&self, timeout: Duration) -> usize {
        let deadline = time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            let notified = self.active_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let remaining = self.active_requests.load(Ordering::SeqCst);
            if remaining == 0 {
                return 0;
            }
            tokio::select! {
                _ = &mut deadline => return remaining,
                _ = notified => {}
            }
        }
    }

    fn finish_request(&self) {
        if self.active_requests.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.active_notify.notify_waiters();
        }
    }
}

pub(super) struct ActiveRequestGuard {
    control: RuntimeControl,
}

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        self.control.finish_request();
    }
}

pub(super) struct RequestLeaseGuard {
    #[allow(dead_code)]
    inner: RequestLeaseGuardKind,
}

/// Backend-specific request-lease guard. The Postgres/Valkey heartbeat
/// stays in this module; the standalone SQLite lease lives in the
/// sibling `lifecycle_standalone` module and is wrapped here so the
/// caller only needs to know the abstract `RequestLeaseGuard` type.
pub(super) enum RequestLeaseGuardKind {
    #[allow(dead_code)]
    Managed(Box<ManagedLeaseGuard>),
    #[allow(dead_code)]
    Standalone(super::lifecycle_standalone::StandaloneRequestLeaseGuard),
}

pub(super) struct ManagedLeaseGuard {
    handle: JoinHandle<()>,
    request_id: Uuid,
    admin_state: Option<AdminState>,
}

impl RequestLeaseGuard {
    pub(super) fn spawn(
        admin_state: Option<&AdminState>,
        request_id: Uuid,
        control: RuntimeControl,
    ) -> Option<Self> {
        let state = admin_state?.clone();
        let handle = tokio::spawn(async move {
            let valkey_enabled = state.replay_cache.enabled();
            if valkey_enabled {
                let (worker_instance_id, lease_expires_at, last_heartbeat_at) =
                    control.current_lease();
                if let Err(err) = state
                    .replay_cache
                    .write_request_lease(
                        request_id,
                        worker_instance_id,
                        (lease_expires_at - last_heartbeat_at).num_seconds().max(1) as u64,
                    )
                    .await
                {
                    warn!(error = %err, %request_id, "failed to initialize valkey request lease");
                }
            }
            let mut interval = time::interval(Duration::from_secs(
                super::REQUEST_RECORD_HEARTBEAT_SECONDS as u64,
            ));
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            interval.tick().await;

            loop {
                interval.tick().await;
                let (worker_instance_id, lease_expires_at, last_heartbeat_at) =
                    control.current_lease();
                if valkey_enabled {
                    match state
                        .replay_cache
                        .refresh_request_lease(
                            request_id,
                            (lease_expires_at - last_heartbeat_at).num_seconds().max(1) as u64,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            warn!(
                                %request_id,
                                %worker_instance_id,
                                "request valkey lease missing during heartbeat refresh"
                            );
                        }
                        Err(err) => {
                            warn!(error = %err, %request_id, "failed to refresh valkey request lease");
                        }
                    }
                }
                match db::heartbeat_request_record_lease(
                    &state.lease_pool,
                    request_id,
                    Some(worker_instance_id),
                    lease_expires_at,
                    last_heartbeat_at,
                )
                .await
                {
                    Ok(0) => {
                        warn!(
                            %request_id,
                            %worker_instance_id,
                            "stopped request lease heartbeat because request record is no longer active"
                        );
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(error = %err, %request_id, "failed to heartbeat request record lease");
                    }
                }
            }
        });
        Some(Self {
            inner: RequestLeaseGuardKind::Managed(Box::new(ManagedLeaseGuard {
                handle,
                request_id,
                admin_state: admin_state.cloned(),
            })),
        })
    }

    /// Construct a guard that owns a standalone SQLite lease. Used by
    /// the runtime dispatcher in `runtime/mod.rs` when the standalone
    /// path is active; this keeps the existing `RequestLeaseGuard` API
    /// surface unchanged for PostgreSQL callers.
    pub(super) fn standalone(
        guard: super::lifecycle_standalone::StandaloneRequestLeaseGuard,
    ) -> Self {
        Self {
            inner: RequestLeaseGuardKind::Standalone(guard),
        }
    }
}

impl Drop for ManagedLeaseGuard {
    fn drop(&mut self) {
        self.handle.abort();
        if let Some(state) = self.admin_state.clone() {
            let request_id = self.request_id;
            tokio::spawn(async move {
                if let Err(err) =
                    db::delete_request_record_lease(&state.lease_pool, request_id).await
                {
                    warn!(error = %err, %request_id, "failed to delete request record lease");
                }
                if state.replay_cache.enabled()
                    && let Err(err) = state.replay_cache.delete_request_lease(request_id).await
                {
                    warn!(error = %err, %request_id, "failed to delete valkey request lease");
                }
            });
        }
    }
}

/// Progress of one request-lease reconcile pass. Recorded between stages so a
/// failure can be logged with the request count and the backend the failing
/// stage used, instead of blaming valkey for what is usually a saturated
/// Postgres lease pool.
struct LeaseReconcileDiagnostics {
    /// Active request records read from the lease pool before the failure.
    active_requests: usize,
    /// Backend the failing stage talked to: the Postgres lease pool or valkey.
    pool_source: &'static str,
    /// Pool-wait counters sampled when the pass started, so the failure log
    /// can report how much contention this pass saw.
    pool_wait_start: db::PoolWaitSnapshot,
}

impl Default for LeaseReconcileDiagnostics {
    fn default() -> Self {
        Self {
            active_requests: 0,
            pool_source: "lease_pool",
            pool_wait_start: db::PoolWaitSnapshot::default(),
        }
    }
}

pub(super) async fn abort_stale_requests_once(admin_state: Option<&AdminState>) {
    let Some(state) = admin_state else {
        return;
    };
    if state.replay_cache.enabled() {
        let mut diagnostics = LeaseReconcileDiagnostics::default();
        match abort_requests_missing_valkey_leases(state, &mut diagnostics).await {
            Ok(count) if count > 0 => {
                warn!(
                    count,
                    abort_reason = "valkey_lease_missing",
                    "aborted active request records missing valkey leases"
                );
            }
            Ok(_) => {}
            Err(err) => {
                let pool_wait = db::pool_wait_snapshot().since(diagnostics.pool_wait_start);
                warn!(
                    error = %err,
                    pool_source = diagnostics.pool_source,
                    active_requests = diagnostics.active_requests,
                    pool_wait_ms = pool_wait.total_wait_ms,
                    pool_slow_acquires = pool_wait.slow_acquires,
                    "failed to reconcile requests against valkey leases"
                );
            }
        }
        return;
    }
    match db::abort_stale_request_records(&state.lease_pool).await {
        Ok(count) if count > 0 => {
            warn!(
                count,
                abort_reason = "worker_lease_expired",
                "aborted stale leased request records"
            );
        }
        Ok(_) => {}
        Err(err) => {
            warn!(error = %err, "failed to abort stale leased request records");
        }
    }
}

pub(super) fn spawn_stale_request_reconciler(
    admin_state: Option<&AdminState>,
    control: RuntimeControl,
) -> Option<JoinHandle<()>> {
    let state = admin_state?.clone();
    Some(tokio::spawn(async move {
        tokio::select! {
            _ = control.wait_for_shutdown() => return,
            _ = time::sleep(Duration::from_secs(super::STALE_REQUEST_SWEEP_SECONDS as u64)) => {}
        }
        let mut interval = time::interval(Duration::from_secs(
            super::STALE_REQUEST_SWEEP_SECONDS as u64,
        ));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = interval.tick() => {
                    if state.replay_cache.enabled() {
                        let mut diagnostics = LeaseReconcileDiagnostics::default();
                        match abort_requests_missing_valkey_leases(&state, &mut diagnostics).await {
                            Ok(count) if count > 0 => {
                                warn!(
                                    count,
                                    abort_reason = "valkey_lease_missing",
                                    "background-aborted request records missing valkey leases"
                                );
                            }
                            Ok(_) => {}
                            Err(err) => {
                                let pool_wait =
                                    db::pool_wait_snapshot().since(diagnostics.pool_wait_start);
                                warn!(
                                    error = %err,
                                    pool_source = diagnostics.pool_source,
                                    active_requests = diagnostics.active_requests,
                                    pool_wait_ms = pool_wait.total_wait_ms,
                                    pool_slow_acquires = pool_wait.slow_acquires,
                                    "failed to reconcile request leases"
                                );
                            }
                        }
                        continue;
                    }
                    match db::abort_stale_request_records(&state.lease_pool).await {
                        Ok(count) if count > 0 => {
                            warn!(
                                count,
                                abort_reason = "worker_lease_expired",
                                "background-aborted stale leased request records"
                            );
                        }
                        Ok(_) => {}
                        Err(err) => {
                            warn!(error = %err, "failed to reconcile stale leased request records");
                        }
                    }
                }
            }
        }
    }))
}

async fn abort_requests_missing_valkey_leases(
    state: &AdminState,
    diagnostics: &mut LeaseReconcileDiagnostics,
) -> anyhow::Result<u64> {
    diagnostics.pool_source = "lease_pool";
    diagnostics.pool_wait_start = db::pool_wait_snapshot();
    let request_ids = db::list_active_request_record_ids(&state.lease_pool).await?;
    diagnostics.active_requests = request_ids.len();
    const VALKEY_LEASE_CHECK_CONCURRENCY: usize = 32;
    let replay_cache = state.replay_cache.clone();
    diagnostics.pool_source = "valkey";
    let checked = futures::stream::iter(request_ids.into_iter().map(|request_id| {
        let replay_cache = replay_cache.clone();
        async move {
            let exists = replay_cache
                .request_lease_exists(request_id)
                .await?
                .unwrap_or(false);
            Ok::<_, anyhow::Error>((request_id, exists))
        }
    }))
    .buffer_unordered(VALKEY_LEASE_CHECK_CONCURRENCY)
    .try_collect::<Vec<_>>()
    .await?;
    let missing = checked
        .into_iter()
        .filter_map(|(request_id, exists)| (!exists).then_some(request_id))
        .collect::<Vec<_>>();
    diagnostics.pool_source = "lease_pool";
    db::abort_request_records_by_ids(&state.lease_pool, &missing).await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::RuntimeControl;

    #[tokio::test]
    async fn drain_returns_once_the_last_guard_drops() {
        let control = RuntimeControl::new();
        let guard = control.try_track_request().expect("admission is open");

        let drain = tokio::spawn({
            let control = control.clone();
            async move { control.wait_for_drain(Duration::from_secs(5)).await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(guard);

        assert_eq!(drain.await.expect("drain task ran"), 0);
    }

    #[tokio::test]
    async fn drain_reports_remaining_requests_at_the_deadline() {
        let control = RuntimeControl::new();
        let _guard = control.try_track_request().expect("admission is open");

        let remaining = control.wait_for_drain(Duration::from_millis(20)).await;

        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn admission_shutdown_rejects_new_guards_but_keeps_the_admitted_round() {
        let control = RuntimeControl::new();
        let guard = control.try_track_request().expect("admission is open");

        control.begin_shutdown();
        assert!(control.try_track_request().is_none());
        assert_eq!(
            control.wait_for_drain(Duration::from_millis(20)).await,
            1,
            "the admitted round still holds the drain open"
        );

        drop(guard);
        assert_eq!(control.wait_for_drain(Duration::from_secs(1)).await, 0);
    }

    #[tokio::test]
    async fn session_stop_stays_independent_from_admission_shutdown() {
        let control = RuntimeControl::new();
        control.begin_shutdown();
        assert!(!control.is_session_stopped());

        let waiting = tokio::spawn({
            let control = control.clone();
            async move { control.wait_for_session_stop().await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !waiting.is_finished(),
            "session stop has not been signalled"
        );

        control.begin_session_stop();
        waiting.await.expect("session stop wait ran");
        assert!(control.is_session_stopped());
    }
}
