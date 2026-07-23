use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use chrono::{DateTime, Utc};
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
    active_requests: Arc<AtomicUsize>,
    active_notify: Arc<Notify>,
}

impl RuntimeControl {
    pub(super) fn new() -> Self {
        Self {
            worker_instance_id: Uuid::new_v4(),
            shutting_down: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
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
        if !self.shutting_down.swap(true, Ordering::SeqCst) {
            self.shutdown_notify.notify_waiters();
            self.active_notify.notify_waiters();
        }
    }

    pub(super) async fn wait_for_shutdown(&self) {
        if self.is_shutting_down() {
            return;
        }
        self.shutdown_notify.notified().await;
    }

    pub(super) fn current_lease(&self) -> (Uuid, DateTime<Utc>, DateTime<Utc>) {
        let last_heartbeat_at = Utc::now();
        let lease_expires_at =
            last_heartbeat_at + chrono::Duration::seconds(super::REQUEST_RECORD_LEASE_SECONDS);
        (self.worker_instance_id, lease_expires_at, last_heartbeat_at)
    }

    pub(super) fn try_track_request(&self) -> Option<ActiveRequestGuard> {
        if self.is_shutting_down() {
            return None;
        }
        self.active_requests.fetch_add(1, Ordering::SeqCst);
        if self.is_shutting_down() {
            self.finish_request();
            return None;
        }
        Some(ActiveRequestGuard {
            control: self.clone(),
        })
    }

    pub(super) async fn wait_for_drain(&self, timeout: Duration) {
        if self.active_requests.load(Ordering::SeqCst) == 0 {
            return;
        }

        let deadline = time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            if self.active_requests.load(Ordering::SeqCst) == 0 {
                return;
            }
            tokio::select! {
                _ = &mut deadline => return,
                _ = self.active_notify.notified() => {}
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
            handle,
            request_id,
            admin_state: admin_state.cloned(),
        })
    }
}

impl Drop for RequestLeaseGuard {
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

pub(super) async fn abort_stale_requests_once(admin_state: Option<&AdminState>) {
    let Some(state) = admin_state else {
        return;
    };
    if state.replay_cache.enabled() {
        match abort_requests_missing_valkey_leases(state).await {
            Ok(count) if count > 0 => {
                warn!(
                    count,
                    "aborted active request records missing valkey leases"
                );
            }
            Ok(_) => {}
            Err(err) => {
                warn!(error = %err, "failed to reconcile requests against valkey leases");
            }
        }
        return;
    }
    match db::abort_stale_request_records(&state.lease_pool).await {
        Ok(count) if count > 0 => {
            warn!(count, "aborted stale leased request records");
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
        let mut interval = time::interval(Duration::from_secs(
            super::STALE_REQUEST_SWEEP_SECONDS as u64,
        ));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = interval.tick() => {
                    if state.replay_cache.enabled() {
                        match abort_requests_missing_valkey_leases(&state).await {
                            Ok(count) if count > 0 => {
                                warn!(count, "background-aborted request records missing valkey leases");
                            }
                            Ok(_) => {}
                            Err(err) => {
                                warn!(error = %err, "failed to reconcile request leases from valkey");
                            }
                        }
                        continue;
                    }
                    match db::abort_stale_request_records(&state.lease_pool).await {
                        Ok(count) if count > 0 => {
                            warn!(count, "background-aborted stale leased request records");
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

async fn abort_requests_missing_valkey_leases(state: &AdminState) -> anyhow::Result<u64> {
    let request_ids = db::list_active_request_record_ids(&state.lease_pool).await?;
    let mut missing = Vec::new();
    for request_id in request_ids {
        if !state
            .replay_cache
            .request_lease_exists(request_id)
            .await?
            .unwrap_or(false)
        {
            missing.push(request_id);
        }
    }
    db::abort_request_records_by_ids(&state.lease_pool, &missing).await
}
