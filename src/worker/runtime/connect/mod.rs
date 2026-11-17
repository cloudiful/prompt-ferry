mod backoff;
mod config;
mod handshake;
mod session;
mod supervisor;
mod support;

use super::{
    STALE_REQUEST_SWEEP_SECONDS, WorkerRuntimeState, WorkerShutdown, build_admin_state,
    build_standalone_state,
    inbound::close_realtime_sessions,
    lifecycle::{join_owned_tasks, stop_accepted_tasks},
    lifecycle_standalone::spawn_standalone_stale_lease_reconciler,
    validate_config,
};
use crate::{
    config::{self as app_config, WorkerConfig},
    runtime_env,
};
use anyhow::Context;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
pub(super) use support::is_expected_relay_disconnect;
use support::{SESSION_TEARDOWN_ALLOWANCE, shutdown_signal};
use tokio::task::JoinSet;
use tracing::{info, warn};

use self::{
    config::{first_relay_connection_config, relay_connection_configs},
    session::connect_once,
    supervisor::spawn_relay_supervisor,
};

/// Adopt the admin socket the startup entrypoint reserved.
///
/// A role-driven process owns the relay, the worker, and the management UI, so
/// it resolves and binds every address it serves before the worker starts.
/// Taking that socket over here — before the database migration and bootstrap
/// that follow — means a fixed admin port cannot be claimed by another process
/// in between, and any bind problem fails the run rather than leaving the
/// process without its UI.
///
/// Returns `None` for a standalone worker, which installs no hand-off and
/// keeps binding its own admin socket inside the admin server.
fn bind_integrated_admin_listener(
    config: &WorkerConfig,
) -> anyhow::Result<Option<tokio::net::TcpListener>> {
    if !app_config::integrated_startup::is_installed() {
        return Ok(None);
    }
    let reserved =
        app_config::integrated_startup::take_reserved_admin_listener().ok_or_else(|| {
            anyhow::anyhow!(
                "integrated startup reserved no admin listener, so an admin bind failure could not \
             be reported to the integrated runner"
            )
        })?;
    reserved
        .set_nonblocking(true)
        .context("failed to switch the reserved admin listener to non-blocking mode")?;
    let listener = tokio::net::TcpListener::from_std(reserved)
        .context("failed to adopt the reserved admin listener")?;

    let bound_addr = listener
        .local_addr()
        .context("failed to read the address of the reserved admin listener")?;
    info!(
        %bound_addr,
        configured_bind = %config.admin_bind,
        "adopted the admin listener reserved by the startup entrypoint"
    );
    app_config::integrated_startup::publish_bound(bound_addr);
    Ok(Some(listener))
}

/// Upper bound for the startup reconciliation of the stale request records.
const STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound for the graceful completion of the admin server.
const ADMIN_SHUTDOWN_JOIN_ALLOWANCE: Duration = Duration::from_secs(15);

/// Upper bound for the tracked admin handler count to reach zero after the
/// listener stopped.
const ADMIN_EXECUTION_JOIN_ALLOWANCE: Duration = Duration::from_secs(5);

/// Upper bound for the last wait of the accepted work.
///
/// The registry row is only released when every accepted execution stopped
/// inside this window.
const FINAL_DRAIN_WAIT: Duration = Duration::from_secs(10);

/// Outcome of the drain of the accepted work.
enum DrainOutcome {
    /// The drain reached its budget with the reported number of requests
    /// still running.
    Drained(usize),
    /// The worker escalated to a fail-closed shutdown during the drain.
    FailClosed,
}

/// Serializes the accepted-task stop passes.
///
/// Exactly one future owns the accepted handle set at a time. The fail-closed
/// monitor and the ordinary shutdown path may both observe an escalation, but
/// the second pass starts only after the first released the gate and then
/// finds the set empty.
#[derive(Clone, Default)]
pub(super) struct AcceptedStopGate {
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl AcceptedStopGate {
    pub(super) async fn stop(&self, runtime_state: &WorkerRuntimeState, allowance: Duration) {
        let _gate = self.lock.lock().await;
        stop_accepted_tasks(runtime_state, allowance).await;
    }
}

/// The single owner of the fail-closed escalation for the whole worker life.
///
/// The monitor is spawned when the services start and is not part of any set
/// that the shutdown drains. Its future owns the accepted-task stop pass and
/// closes admin handler admission, and no `select!` branch can drop it while
/// it owns handles: the shutdown only notifies it and then joins the same
/// owned task. It stays alive through the relay and admin joins, so an
/// escalation during those joins still cancels admin business execution.
///
/// Lifetime: `run_embedded` keeps the value (`stop_tx` and the owned
/// `JoinHandle`) until every business execution stopped, then calls `finish`.
/// Dropping the value before `finish` would only detach a monitor that owns no
/// handles: the accepted handles live in `WorkerRuntimeState` until a stop
/// pass takes them.
pub(super) struct FailClosedMonitor {
    stop_tx: tokio::sync::watch::Sender<bool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl FailClosedMonitor {
    pub(super) fn spawn(
        runtime_state: WorkerRuntimeState,
        admin: Option<crate::worker_admin::state::AdminExecutionTracker>,
        stop_gate: AcceptedStopGate,
    ) -> Self {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let control = runtime_state.control.clone();
        let handle = tokio::spawn(async move {
            let escalation = async {
                control.wait_for_fail_closed().await;
            };
            let stop = async {
                while !*stop_rx.borrow() {
                    if stop_rx.changed().await.is_err() {
                        return;
                    }
                }
            };
            tokio::select! {
                _ = escalation => {}
                _ = stop => {}
            }
            // The stop notification may win the select in the same instant the
            // escalation latches, so the terminal state is checked instead of
            // trusting the branch.
            if runtime_state.control.is_fail_closed() {
                if let Some(tracker) = &admin {
                    tracker.cancel();
                }
                cancel_in_flight_requests(&runtime_state).await;
                stop_gate.stop(&runtime_state, FINAL_DRAIN_WAIT).await;
            }
        });
        Self {
            stop_tx,
            handle: Some(handle),
        }
    }

    /// Stop and join the monitor after every business execution stopped.
    pub(super) async fn finish(mut self) {
        let _ = self.stop_tx.send(true);
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    /// The owned monitor task, for orchestration tests that must observe it
    /// without releasing its ownership.
    #[cfg(test)]
    pub(super) fn handle_mut(&mut self) -> &mut tokio::task::JoinHandle<()> {
        self.handle.as_mut().expect("the monitor task is owned")
    }
}

/// The admin-execution accounting of one worker.
///
/// It is built from the actual admin server state, which exists for every
/// supported backend (PostgreSQL and SQLite). The PostgreSQL-only runtime
/// state is deliberately not an input: a SQLite worker still runs the same
/// admin server, so its handler executions must reach the same monitor,
/// cancellation and terminal idle check.
#[derive(Clone, Default)]
pub(super) struct AdminExecutionLifecycle {
    tracker: Option<crate::worker_admin::state::AdminExecutionTracker>,
}

impl AdminExecutionLifecycle {
    pub(super) fn new(
        admin_state: Option<&crate::worker_admin::AdminState>,
    ) -> AdminExecutionLifecycle {
        AdminExecutionLifecycle {
            tracker: admin_state.map(|state| state.admin_executions.clone()),
        }
    }

    /// The tracker handed to the fail-closed monitor.
    pub(super) fn monitor_tracker(
        &self,
    ) -> Option<crate::worker_admin::state::AdminExecutionTracker> {
        self.tracker.clone()
    }

    /// Close admin handler admission for the current and future executions.
    pub(super) fn cancel(&self) {
        if let Some(tracker) = &self.tracker {
            tracker.cancel();
        }
    }

    /// Wait for tracked admin handlers to stop, independently of the backend.
    pub(super) async fn wait_for_idle(&self, budget: Duration) -> usize {
        match &self.tracker {
            Some(tracker) => tracker.wait_for_idle(budget).await,
            None => 0,
        }
    }

    /// The SQLite-style wiring used by the regression test: the real admin
    /// server state supplied its tracker while no PostgreSQL runtime state
    /// exists in the worker process.
    #[cfg(test)]
    pub(super) fn without_postgres_runtime_state(
        tracker: crate::worker_admin::state::AdminExecutionTracker,
    ) -> AdminExecutionLifecycle {
        AdminExecutionLifecycle {
            tracker: Some(tracker),
        }
    }
}

pub(super) async fn run_embedded(
    config: WorkerConfig,
    external_shutdown: Option<tokio::sync::watch::Receiver<super::ExternalStop>>,
) -> anyhow::Result<()> {
    validate_config(&config)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .build()
        .context("failed to build upstream HTTP client")?;
    // Resolve and validate every local relay target before the worker claims
    // its registry row or starts a service. An unreadable CA/client PEM file
    // or invalid TLS material then fails the start while the worker owns
    // nothing yet, instead of after the admin listener and schedulers run.
    let relay_configs = relay_connection_configs(&config)?;
    // Bound before any slow bootstrap work, so an admin listener problem fails
    // startup here instead of being logged by a detached task much later.
    let admin_listener = bind_integrated_admin_listener(&config)?;
    let worker_shutdown = WorkerShutdown::new();
    // The state is built without starting any service: the admin listener,
    // the schedulers and the relay bridge only start after the worker was
    // admitted to the registry.
    let admin_state = build_admin_state(&config, None).await?;
    // The shared settings are resolved from the database before any storage
    // contract, cache or coordinated scheduler is built.
    let shared_valkey = admin_state
        .as_ref()
        .map(|state| state.shared_valkey.clone())
        .unwrap_or_else(crate::db::SharedValkeyConfig::not_configured);
    let contract = config.storage_contract(shared_valkey.url_or_empty());
    if contract.backend.is_postgres() {
        info!(
            backend = contract.backend.as_str(),
            coordinator = contract.coordinator.as_str(),
            shared_valkey_configured = shared_valkey.is_configured(),
            shared_valkey_revision = ?shared_valkey.revision(),
            "worker storage mode selected; configured PostgreSQL is authoritative"
        );
    } else {
        let sqlite_path =
            runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
        info!(
            backend = contract.backend.as_str(),
            coordinator = contract.coordinator.as_str(),
            sqlite_path = %sqlite_path.display(),
            bootstrap = "static-relay-and-upstream",
            "worker storage mode selected; standalone SQLite configuration store is authoritative"
        );
    }
    let runtime_admin_state = if contract.backend.is_postgres() {
        admin_state.clone()
    } else {
        None
    };
    // Admin handler executions belong to the admin server, which every backend
    // starts. This lifecycle must never be derived from the PostgreSQL-only
    // runtime state: a SQLite worker needs the same cancellation and terminal
    // idle accounting as a PostgreSQL one.
    let admin_lifecycle = AdminExecutionLifecycle::new(admin_state.as_ref());
    // Every remaining fallible constructor runs before the admission and
    // before any service task exists. A failure here therefore leaves no
    // registry row and no owned service task behind.
    let standalone_state = if contract.backend.is_postgres() {
        None
    } else {
        Some(
            build_standalone_state(
                &config,
                None,
                admin_state
                    .as_ref()
                    .map(|state| state.mcp_catalog_cache.clone()),
            )
            .await?,
        )
    };
    // The coordinated schedulers need the shared Valkey. A configured
    // coordinator that cannot be reached must stop the worker before it
    // starts serving, and the store the consumers run with is the one that
    // was initialized here.
    let coordinated_store = if shared_valkey.is_configured() {
        Some(
            super::background_job::connect_coordinated_store(shared_valkey.url_or_empty())
                .await
                .ok_or_else(|| {
                    anyhow::anyhow!("the configured shared Valkey coordination is not available")
                })?,
        )
    } else {
        None
    };
    let runtime_state = WorkerRuntimeState::default();
    // One internal shutdown path: an OS signal and an internal failure stop
    // the same services, so the admin listener, the relay bridge and the
    // schedulers never keep running next to a worker that stopped.
    runtime_state
        .control
        .attach_shutdown_broadcast(worker_shutdown.clone());
    // The caller can ask the worker to stop: the integrated role uses the
    // signal when its relay half ends, so the two halves never keep running
    // apart. A failed other half stops the worker fail closed, because the
    // worker cannot serve the bridge any more.
    let mut external_shutdown_task = None;
    if let Some(mut external_shutdown) = external_shutdown {
        let external_state = runtime_state.clone();
        external_shutdown_task = Some(tokio::spawn(async move {
            let stop = if *external_shutdown.borrow() == super::super::ExternalStop::FailClosed {
                // The signal was already sent before the worker observed it.
                super::super::ExternalStop::FailClosed
            } else {
                let _ = external_shutdown.changed().await;
                *external_shutdown.borrow()
            };
            match stop {
                super::super::ExternalStop::Graceful => external_state.begin_shutdown(),
                super::super::ExternalStop::FailClosed => {
                    external_state.control.begin_fail_closed_shutdown()
                }
            }
        }));
    }
    // The admission is the gate of the whole worker: the identity, the
    // deployment-wide coordination mode and the applied shared revision are
    // claimed before the admin listener, the schedulers and the relay bridge
    // start. A rejected admission fails the startup instead of letting the
    // worker serve next to an incompatible or already-owned process.
    let mut registry_stop = None;
    let mut registry_task = None;
    let mut lease_watchdog = None;
    let coordination = if shared_valkey.is_configured() {
        crate::db::CoordinationSnapshot::shared(shared_valkey.revision())
    } else {
        crate::db::CoordinationSnapshot::bootstrap(shared_valkey.revision())
    };
    if let Some(state) = runtime_admin_state.as_ref() {
        let admitted_at = match super::registry::admit_worker(
            &state.pool,
            &config,
            &runtime_state.control,
            coordination,
        )
        .await
        {
            Ok(admitted_at) => admitted_at,
            Err(err) => {
                abandon_startup(
                    &mut registry_stop,
                    &mut registry_task,
                    &mut lease_watchdog,
                    &mut external_shutdown_task,
                    runtime_admin_state.as_ref(),
                    &config,
                    &runtime_state.control,
                    &worker_shutdown,
                )
                .await;
                return Err(err);
            }
        };
        // The heartbeat and the lease watchdog start with the admission: the
        // registry lease covers the whole startup, not only the time after
        // the services came up.
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let tracker = super::registry::LeaseTracker::new();
        lease_watchdog = Some(super::registry::spawn_lease_watchdog(
            &runtime_state.control,
            admitted_at,
            tracker.clone(),
            stop_rx.clone(),
        ));
        registry_task = Some(super::registry::spawn_worker_registry(
            state.pool.clone(),
            &config,
            &runtime_state.control,
            coordination.revision,
            coordination.mode,
            admitted_at,
            tracker,
            stop_rx,
        ));
        registry_stop = Some(stop_tx);
    }
    // The stale request reconciliation runs before the listener starts, so
    // the worker does not serve while it is still reconciling. It is
    // bounded: a slow database must not hold the startup past the registry
    // lease, and the periodic reconciler repeats the pass anyway.
    if tokio::time::timeout(
        STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT,
        super::abort_stale_requests_once(runtime_admin_state.as_ref()),
    )
    .await
    .is_err()
    {
        warn!(
            timeout_seconds = STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT.as_secs(),
            "the startup request reconciliation did not finish in time; continuing"
        );
    }
    if runtime_state.control.is_shutting_down() {
        abandon_startup(
            &mut registry_stop,
            &mut registry_task,
            &mut lease_watchdog,
            &mut external_shutdown_task,
            runtime_admin_state.as_ref(),
            &config,
            &runtime_state.control,
            &worker_shutdown,
        )
        .await;
        return Err(anyhow::anyhow!(
            "the worker stopped during startup before it could serve"
        ));
    }
    // The admin listener starts only after the admission succeeded, and every
    // handle below is owned by the single teardown path.
    let admin_task = admin_state.as_ref().map(|state| {
        super::bootstrap::start_admin_server(
            state.clone(),
            &config,
            admin_listener,
            Some(&worker_shutdown),
        )
    });
    let stale_reconciler = super::spawn_stale_request_reconciler(
        runtime_admin_state.as_ref(),
        runtime_state.control.clone(),
    );
    let standalone_stale_reconciler = spawn_standalone_stale_lease_reconciler(
        standalone_state.clone(),
        runtime_state.control.clone(),
        Duration::from_secs(STALE_REQUEST_SWEEP_SECONDS.max(1) as u64),
    );
    let raw_maintenance_task = if let Some(state) = runtime_admin_state.as_ref() {
        Some(super::raw_maintenance::spawn(
            &shared_valkey,
            state.pool.clone(),
            state.usage_retention.clone(),
            state.raw_payload_store.clone(),
            runtime_state.control.clone(),
            coordinated_store.clone(),
        ))
    } else {
        None
    };
    let cache_alert_task = if let Some(state) = runtime_admin_state.as_ref() {
        Some(super::cache_alert_monitor::spawn_cache_alert_monitor(
            &shared_valkey,
            state.pool.clone(),
            state.cache_alert.clone(),
            client.clone(),
            runtime_state.control.clone(),
            coordinated_store.clone(),
        ))
    } else {
        None
    };
    let chatgpt_quota_task = runtime_admin_state.as_ref().and_then(|state| {
        state.chatgpt_quota_service.clone().map(|service| {
            super::chatgpt_quota_collector::spawn(
                &shared_valkey,
                service,
                runtime_state.control.clone(),
                coordinated_store.clone(),
            )
        })
    });
    let quota_observation_task = runtime_admin_state.as_ref().map(|state| {
        super::quota_observation_collector::spawn(
            &shared_valkey,
            state.pool.clone(),
            state.token_plan_quota.clone(),
            runtime_state.control.clone(),
            coordinated_store.clone(),
        )
    });
    // The catalog refresh lifecycle has exactly one owner: this task replaces
    // the former startup warmup and adds the bounded periodic pass.
    let mcp_catalog_refresh_task = admin_state.as_ref().map(|state| {
        super::mcp_catalog_refresh::spawn(
            &config,
            state.mcp_catalog_service.clone(),
            runtime_state.control.clone(),
        )
    });
    // The database-backed settings are re-read on a bounded interval so a
    // change made by another worker reaches this process as well.
    let config_refresh_task = runtime_admin_state.as_ref().map(|state| {
        super::config_refresh::spawn_config_refresh(
            state.clone(),
            config.clone(),
            runtime_state.control.clone(),
        )
    });
    // The OS signal uses the same internal shutdown path as an internal
    // failure, so both stop the admin listener, the bridge and the
    // schedulers. The task is owned here and aborted once the worker
    // stopped, so its sender cannot keep the admin server alive.
    let shutdown_state = runtime_state.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_state.begin_shutdown();
    });

    // The supervisor itself cannot fail: the relay targets were resolved and
    // validated before the worker was admitted, so no startup error can leave
    // the admin listener or a scheduler running without an owned registry row.
    let mut relay_tasks = JoinSet::new();
    spawn_relay_supervisor(
        relay_configs,
        config.clone(),
        client.clone(),
        admin_state.clone(),
        standalone_state.clone(),
        runtime_state.clone(),
        &mut relay_tasks,
    );

    // The fail-closed monitor lives from service start through the relay and
    // admin joins below. It is the only future that aborts and joins accepted
    // task handles on escalation, so no shutdown select can drop a future that
    // still owns handles.
    let accepted_stop_gate = AcceptedStopGate::default();
    let fail_closed_monitor = FailClosedMonitor::spawn(
        runtime_state.clone(),
        admin_lifecycle.monitor_tracker(),
        accepted_stop_gate.clone(),
    );

    // Accepted rounds keep their bridge session and writer while they drain.
    runtime_state.wait_for_shutdown().await;
    // A worker that lost its fencing or its registry lease must not drain: the
    // monitor has already stopped its accepted and admin work, and the drain
    // budget is zero.
    let mut fail_closed = runtime_state.control.is_fail_closed();
    let drain_budget = super::lifecycle::RuntimeControl::shutdown_drain_budget(
        fail_closed,
        config.shutdown_drain_seconds,
    );
    let shutdown_deadline = tokio::time::Instant::now() + drain_budget;
    info!(
        budget_seconds = drain_budget.as_secs(),
        fail_closed, "worker shutdown admission closed; draining accepted request rounds"
    );
    // The pre-drain closes the realtime sessions. It is bounded and it is
    // interrupted by a fail-closed escalation.
    {
        let pre_drain = async {
            tokio::select! {
                _ = tokio::time::timeout_at(
                    shutdown_deadline,
                    close_realtime_sessions(&runtime_state, shutdown_deadline),
                ) => {}
                _ = runtime_state.control.wait_for_fail_closed() => {}
            }
        };
        pre_drain.await;
    }
    fail_closed = fail_closed || runtime_state.control.is_fail_closed();

    // The background tasks are stopped with a bounded allowance. A task that
    // does not stop inside the allowance is aborted and joined: a detached
    // task could still run when the registry row is released.
    let mut background_tasks: Vec<tokio::task::JoinHandle<()>> = [
        raw_maintenance_task,
        cache_alert_task,
        chatgpt_quota_task,
        quota_observation_task,
        mcp_catalog_refresh_task,
        config_refresh_task,
        stale_reconciler,
        standalone_stale_reconciler,
    ]
    .into_iter()
    .flatten()
    .collect();
    // One join pass owns every background task. The escalation and the
    // deadline are handled inside that pass, so a completed handle is consumed
    // once and never awaited again by a second escalation path.
    let maintenance_wait = join_owned_tasks(
        &mut background_tasks,
        SESSION_TEARDOWN_ALLOWANCE,
        &runtime_state.control,
    );
    // The drain observes the fail-closed escalation: a lost fencing, an
    // expiring registry lease or a failed coordinator interrupts the drain
    // instead of letting accepted work run past the lease. Stopping the
    // accepted work itself belongs to the monitor, so this join never owns a
    // handle set.
    let remaining_drain_budget =
        shutdown_deadline.saturating_duration_since(tokio::time::Instant::now());
    let drain = async {
        tokio::select! {
            remaining = runtime_state.control.wait_for_drain(remaining_drain_budget) => {
                DrainOutcome::Drained(remaining)
            }
            _ = runtime_state.control.wait_for_fail_closed() => DrainOutcome::FailClosed,
        }
    };
    let (drain_outcome, ()) = tokio::join!(drain, maintenance_wait);
    fail_closed = fail_closed || runtime_state.control.is_fail_closed();
    let remaining = match drain_outcome {
        DrainOutcome::Drained(remaining) => remaining,
        DrainOutcome::FailClosed => {
            fail_closed = true;
            usize::MAX
        }
    };
    if fail_closed || remaining > 0 {
        warn!(
            remaining,
            budget_seconds = drain_budget.as_secs(),
            fail_closed,
            "worker shutdown drain ended; cancelling the remaining requests"
        );
        cancel_in_flight_requests(&runtime_state).await;
        if !fail_closed {
            // The drain deadline is also an ownership deadline: the accepted
            // tasks are force-aborted before the bridge and admin teardown
            // waits, never after them.
            accepted_stop_gate
                .stop(&runtime_state, Duration::ZERO)
                .await;
        }
    }
    // The pending chunk senders are dropped, so the collectors of the
    // requests that never received their end finish as well. A collector
    // that already finished its body sees the cancellation and does not
    // answer a partially collected request.
    runtime_state.pending_requests.lock().await.clear();
    runtime_state.pending_mcp_requests.lock().await.clear();
    // The registered rounds are drained; close the bridge sessions and give
    // each writer its bounded flush before the supervisors are torn down.
    runtime_state.control.begin_session_stop();
    let supervisors = async { while relay_tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(
        SESSION_TEARDOWN_ALLOWANCE + Duration::from_secs(3),
        supervisors,
    )
    .await
    .is_err()
    {
        warn!(
            allowance_seconds = SESSION_TEARDOWN_ALLOWANCE.as_secs(),
            "relay supervisors did not finish session teardown within the allowance; aborting them",
        );
        relay_tasks.abort_all();
        while relay_tasks.join_next().await.is_some() {}
    }
    // The admin server stops with the worker. Its graceful completion is
    // awaited, so the registry row is not released while an administrative
    // request is still running.
    if let Some(mut task) = admin_task {
        if tokio::time::timeout(ADMIN_SHUTDOWN_JOIN_ALLOWANCE, &mut task)
            .await
            .is_err()
        {
            warn!(
                allowance_seconds = ADMIN_SHUTDOWN_JOIN_ALLOWANCE.as_secs(),
                "the admin server did not finish its graceful shutdown in time; aborting it"
            );
            // The admin server may exist on every backend, so its tracked
            // handlers are cancelled through the admin lifecycle, never
            // through the PostgreSQL-only runtime state.
            admin_lifecycle.cancel();
            task.abort();
            let _ = task.await;
        }
    }
    // An ordinary shutdown runs its single stop pass here. On escalation the
    // monitor owns that pass and is awaited below, so no competing stop future
    // can exist.
    if !runtime_state.control.is_fail_closed() {
        accepted_stop_gate
            .stop(&runtime_state, FINAL_DRAIN_WAIT)
            .await;
    }
    let mut remaining_guards = runtime_state.control.wait_for_drain(Duration::ZERO).await;
    // The listener JoinHandle does not own the Axum connection tasks, so the
    // tracked business handlers are the proof that admin mutations stopped.
    let mut remaining_admin_handlers = admin_lifecycle
        .wait_for_idle(ADMIN_EXECUTION_JOIN_ALLOWANCE)
        .await;
    // The monitor is cancelled and joined only after business execution
    // stopped. Its own stop pass joined every accepted handle it took, and the
    // admin handlers above were awaited through their cancellation.
    fail_closed_monitor.finish().await;
    // An escalation that raced the monitor's stop notification still gets the
    // same terminal treatment before the release decision.
    if runtime_state.control.is_fail_closed() {
        admin_lifecycle.cancel();
        accepted_stop_gate
            .stop(&runtime_state, Duration::ZERO)
            .await;
        remaining_guards = runtime_state.control.wait_for_drain(Duration::ZERO).await;
        remaining_admin_handlers = admin_lifecycle.wait_for_idle(Duration::from_secs(1)).await;
    }
    stop_registry_tasks(&mut registry_stop, &mut registry_task, &mut lease_watchdog).await;
    if remaining_guards == 0 && remaining_admin_handlers == 0 {
        release_admission(
            runtime_admin_state.as_ref(),
            &config,
            &runtime_state.control,
        )
        .await;
    } else {
        warn!(
            remaining_guards,
            remaining_admin_handlers,
            "accepted or admin work did not stop; the worker keeps its registry row until it expires"
        );
    }
    // The admin listener stops with the worker, and the owned signal task is
    // aborted so its sender cannot keep the admin server alive.
    worker_shutdown.trigger();
    signal_task.abort();
    let _ = signal_task.await;
    if let Some(task) = external_shutdown_task {
        task.abort();
        let _ = task.await;
    }
    Ok(())
}

/// Abort a startup before the worker served.
///
/// This is the single cleanup path for every post-admission startup error.
/// Services never start before the last fallible startup step, so this path
/// has no admin, scheduler or accepted task to join: it stops the registry
/// heartbeat and watchdog, releases the admission of this incarnation, and
/// aborts and joins the owned external-shutdown task.
#[allow(clippy::too_many_arguments)]
async fn abandon_startup(
    registry_stop: &mut Option<tokio::sync::watch::Sender<bool>>,
    registry_task: &mut Option<tokio::task::JoinHandle<()>>,
    lease_watchdog: &mut Option<tokio::task::JoinHandle<()>>,
    external_shutdown_task: &mut Option<tokio::task::JoinHandle<()>>,
    admin_state: Option<&crate::worker_admin::AdminState>,
    config: &WorkerConfig,
    control: &super::lifecycle::RuntimeControl,
    worker_shutdown: &WorkerShutdown,
) {
    stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
    release_admission(admin_state, config, control).await;
    worker_shutdown.trigger();
    if let Some(task) = external_shutdown_task.take() {
        task.abort();
        let _ = task.await;
    }
}

/// Stop the registry tasks of a worker.
///
/// The heartbeat stops before the registry row is released, so a heartbeat
/// can never resurrect a row that was already marked stopped.
async fn stop_registry_tasks(
    registry_stop: &mut Option<tokio::sync::watch::Sender<bool>>,
    registry_task: &mut Option<tokio::task::JoinHandle<()>>,
    lease_watchdog: &mut Option<tokio::task::JoinHandle<()>>,
) {
    if let Some(stop_tx) = registry_stop.take() {
        let _ = stop_tx.send(true);
    }
    if let Some(task) = registry_task.take() {
        if let Err(err) = task.await {
            warn!(error = %err, "worker registry task join failed during shutdown");
        }
    }
    if let Some(task) = lease_watchdog.take() {
        if let Err(err) = task.await {
            warn!(error = %err, "worker lease watchdog join failed during shutdown");
        }
    }
}

/// Release the registry admission of a worker that never served.
async fn release_admission(
    admin_state: Option<&crate::worker_admin::AdminState>,
    config: &WorkerConfig,
    control: &super::lifecycle::RuntimeControl,
) {
    let Some(state) = admin_state else {
        return;
    };
    super::registry::release_worker(
        &state.pool,
        &config.resolved_worker_identity(),
        control.worker_instance_id(),
    )
    .await;
}

/// Cancel every still-registered request through the same `RequestCancellation`
/// the bridge cancel path uses. At this point no new request can be admitted,
/// so the remaining set only shrinks.
async fn cancel_in_flight_requests(runtime_state: &WorkerRuntimeState) {
    let requests = {
        let cancellations = runtime_state.request_cancellations.lock().await;
        cancellations.values().cloned().collect::<Vec<_>>()
    };
    let mcp_requests = {
        let cancellations = runtime_state.mcp_request_cancellations.lock().await;
        cancellations.values().cloned().collect::<Vec<_>>()
    };
    for cancellation in requests.into_iter().chain(mcp_requests) {
        cancellation.cancel();
    }
}

pub(super) async fn connect_for_test(config: WorkerConfig, client: Client) -> anyhow::Result<()> {
    let relay = first_relay_connection_config(&config)?;
    connect_once(
        &relay,
        config,
        client,
        None,
        None,
        WorkerRuntimeState::default(),
    )
    .await
}

pub(super) async fn connect_for_test_with_admin(
    config: WorkerConfig,
    client: Client,
) -> anyhow::Result<()> {
    let admin_state = build_admin_state(&config, None).await?;
    let runtime_admin_state = if config.storage_backend().is_postgres() {
        admin_state.clone()
    } else {
        None
    };
    let standalone_state = if config.storage_backend().is_postgres() {
        None
    } else {
        Some(
            build_standalone_state(
                &config,
                None,
                admin_state
                    .as_ref()
                    .map(|state| state.mcp_catalog_cache.clone()),
            )
            .await?,
        )
    };
    let relay = first_relay_connection_config(&config)?;
    connect_once(
        &relay,
        config,
        client,
        runtime_admin_state,
        standalone_state,
        WorkerRuntimeState::default(),
    )
    .await
}
