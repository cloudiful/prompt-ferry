mod backoff;
mod config;
mod handshake;
mod session;
mod supervisor;
mod support;

use super::{
    STALE_REQUEST_SWEEP_SECONDS, WorkerRuntimeState, WorkerShutdown, ai::abort_waiting_approvals,
    build_admin_state, build_standalone_state, inbound::close_realtime_sessions,
    lifecycle_standalone::spawn_standalone_stale_lease_reconciler, validate_config,
};
use crate::{
    config::{self as app_config, WorkerConfig},
    runtime_env,
};
use anyhow::Context;
use reqwest::Client;
use std::time::Duration;
pub(super) use support::is_expected_relay_disconnect;
use support::{SESSION_TEARDOWN_ALLOWANCE, shutdown_signal};
use tokio::task::JoinSet;
use tracing::{info, warn};

use self::{
    config::{RelayConnectionConfig, first_simple_relay_connection_config},
    session::{connect_once, run_relay_loop},
    supervisor::{
        require_managed_admin_state, spawn_managed_relay_supervisor,
        spawn_standalone_relay_supervisor,
    },
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

pub(super) async fn run_embedded(config: WorkerConfig) -> anyhow::Result<()> {
    validate_config(&config)?;
    let contract = config.storage_contract();
    if contract.backend.is_postgres() {
        info!(
            backend = contract.backend.as_str(),
            coordinator = contract.coordinator.as_str(),
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
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .build()
        .context("failed to build upstream HTTP client")?;
    // Bound before any slow bootstrap work, so an admin listener problem fails
    // startup here instead of being logged by a detached task much later.
    let admin_listener = bind_integrated_admin_listener(&config)?;
    let worker_shutdown = WorkerShutdown::new();
    let admin_state =
        build_admin_state(&config, true, None, Some(&worker_shutdown), admin_listener).await?;
    let runtime_admin_state = if contract.backend.is_postgres() {
        admin_state.clone()
    } else {
        None
    };
    let standalone_state = if contract.backend.is_postgres() {
        None
    } else {
        Some(build_standalone_state(&config, None).await?)
    };
    let runtime_state = WorkerRuntimeState::default();
    super::abort_stale_requests_once(runtime_admin_state.as_ref()).await;
    let _stale_reconciler = super::spawn_stale_request_reconciler(
        runtime_admin_state.as_ref(),
        runtime_state.control.clone(),
    );
    let _standalone_stale_reconciler = spawn_standalone_stale_lease_reconciler(
        standalone_state.clone(),
        runtime_state.control.clone(),
        Duration::from_secs(STALE_REQUEST_SWEEP_SECONDS.max(1) as u64),
    );
    let raw_maintenance_task = if let Some(state) = runtime_admin_state.as_ref() {
        Some(super::raw_maintenance::spawn(
            &config,
            state.pool.clone(),
            state.usage_retention.clone(),
            state.raw_payload_store.clone(),
            runtime_state.control.clone(),
        ))
    } else {
        None
    };
    let cache_alert_task = if let Some(state) = runtime_admin_state.as_ref() {
        Some(super::cache_alert_monitor::spawn_cache_alert_monitor(
            &config,
            state.pool.clone(),
            state.cache_alert.clone(),
            client.clone(),
            runtime_state.control.clone(),
        ))
    } else {
        None
    };
    let chatgpt_quota_task = runtime_admin_state.as_ref().and_then(|state| {
        state.chatgpt_quota_service.clone().map(|service| {
            super::chatgpt_quota_collector::spawn(&config, service, runtime_state.control.clone())
        })
    });
    let quota_observation_task = runtime_admin_state.as_ref().map(|state| {
        super::quota_observation_collector::spawn(
            &config,
            state.pool.clone(),
            state.token_plan_quota.clone(),
            runtime_state.control.clone(),
        )
    });
    let shutdown_state = runtime_state.clone();
    let admin_shutdown = worker_shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_state.begin_shutdown();
        admin_shutdown.trigger();
    });

    let mut relay_tasks = JoinSet::new();
    if contract.backend.is_postgres() {
        let state = require_managed_admin_state(runtime_admin_state.as_ref())?;
        spawn_managed_relay_supervisor(
            config.clone(),
            client.clone(),
            state,
            runtime_state.clone(),
            &mut relay_tasks,
        )
        .await?;
    } else {
        let state = standalone_state
            .clone()
            .ok_or_else(|| anyhow::anyhow!("standalone mode requires standalone runtime state"))?;
        spawn_standalone_relay_supervisor(
            config.clone(),
            client.clone(),
            state,
            runtime_state.clone(),
            &mut relay_tasks,
        )
        .await?;
    }

    // Admission is closed, but the accepted rounds keep their bridge session
    // and its writer: only the waiting human approvals and the long-lived
    // realtime sessions stop here, using their existing cancellation paths.
    runtime_state.wait_for_shutdown().await;
    let drain_budget = Duration::from_secs(config.shutdown_drain_seconds.max(1));
    let shutdown_deadline = tokio::time::Instant::now() + drain_budget;
    info!(
        budget_seconds = drain_budget.as_secs(),
        "worker shutdown admission closed; draining accepted request rounds"
    );
    let approvals = async {
        if let Some(state) = runtime_admin_state.as_ref() {
            abort_waiting_approvals(state, &client).await;
        }
    };
    let realtime_close = close_realtime_sessions(&runtime_state, shutdown_deadline);
    if tokio::time::timeout_at(shutdown_deadline, async {
        tokio::join!(approvals, realtime_close);
    })
    .await
    .is_err()
    {
        warn!(
            budget_seconds = drain_budget.as_secs(),
            "worker shutdown pre-drain cleanup reached the shared deadline"
        );
    }

    let maintenance_wait = async {
        let join_background = async {
            if let Some(task) = raw_maintenance_task {
                match task.await {
                    Ok(()) => {}
                    Err(join_error) => warn!(
                        error = %join_error,
                        "raw maintenance task join failed during shutdown",
                    ),
                }
            }
            if let Some(task) = cache_alert_task {
                match task.await {
                    Ok(()) => {}
                    Err(join_error) => warn!(
                        error = %join_error,
                        "cache alert task join failed during shutdown",
                    ),
                }
            }
            if let Some(task) = chatgpt_quota_task {
                match task.await {
                    Ok(()) => {}
                    Err(join_error) => warn!(
                        error = %join_error,
                        "ChatGPT quota collector task join failed during shutdown",
                    ),
                }
            }
            if let Some(task) = quota_observation_task {
                match task.await {
                    Ok(()) => {}
                    Err(join_error) => warn!(
                        error = %join_error,
                        "quota observation collector task join failed during shutdown",
                    ),
                }
            }
        };
        if tokio::time::timeout(SESSION_TEARDOWN_ALLOWANCE, join_background)
            .await
            .is_err()
        {
            warn!(
                allowance_seconds = SESSION_TEARDOWN_ALLOWANCE.as_secs(),
                "background tasks did not stop within the teardown allowance; exiting anyway",
            );
        }
    };
    let remaining_drain_budget =
        shutdown_deadline.saturating_duration_since(tokio::time::Instant::now());
    let (remaining, ()) = tokio::join!(
        runtime_state.control.wait_for_drain(remaining_drain_budget),
        maintenance_wait
    );
    if remaining > 0 {
        warn!(
            remaining,
            budget_seconds = drain_budget.as_secs(),
            "worker shutdown drain deadline reached; cancelling remaining requests",
        );
        cancel_in_flight_requests(&runtime_state).await;
    }

    // The registered rounds are drained; close the bridge sessions and give
    // each writer its bounded flush before the supervisors are torn down.
    runtime_state.control.begin_session_stop();
    let supervisors = async { while relay_tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(
        SESSION_TEARDOWN_ALLOWANCE + Duration::from_secs(1),
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
    Ok(())
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
    let relay = first_simple_relay_connection_config(&config)?;
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
    let admin_state = build_admin_state(&config, false, None, None, None).await?;
    let runtime_admin_state = if config.storage_backend().is_postgres() {
        admin_state.clone()
    } else {
        None
    };
    let standalone_state = if config.storage_backend().is_postgres() {
        None
    } else {
        Some(build_standalone_state(&config, None).await?)
    };
    let relay = first_simple_relay_connection_config(&config)?;
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
