mod backoff;
mod config;
mod handshake;
mod session;
mod supervisor;
mod support;

use super::{
    STALE_REQUEST_SWEEP_SECONDS, WorkerRuntimeState, WorkerShutdown, ai::abort_waiting_approvals,
    build_admin_state, build_standalone_state,
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
use support::shutdown_signal;
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

    runtime_state.wait_for_shutdown().await;
    if let Some(state) = runtime_admin_state.as_ref() {
        abort_waiting_approvals(state, &client).await;
    }
    relay_tasks.abort_all();
    while relay_tasks.join_next().await.is_some() {}

    // Bound the in-flight drain so the process exits inside the compose
    // stop grace even when individual requests are slow. The abort calls
    // above already cancelled anything still being processed; this just
    // waits for the worker bookkeeping to settle. Run both pieces under
    // the same budget so neither blocks the other.
    let drain_budget = Duration::from_secs(config.shutdown_drain_seconds.max(1));
    let maintenance_wait = async {
        if let Some(task) = raw_maintenance_task {
            match tokio::time::timeout(drain_budget, task).await {
                Ok(Ok(())) => {}
                Ok(Err(join_error)) => warn!(
                    error = %join_error,
                    "raw maintenance task join failed during shutdown",
                ),
                Err(_) => warn!(
                    budget_seconds = drain_budget.as_secs(),
                    "raw maintenance task did not stop within drain budget; exiting anyway",
                ),
            }
        }
        if let Some(task) = cache_alert_task {
            match tokio::time::timeout(drain_budget, task).await {
                Ok(Ok(())) => {}
                Ok(Err(join_error)) => warn!(
                    error = %join_error,
                    "cache alert task join failed during shutdown",
                ),
                Err(_) => warn!(
                    budget_seconds = drain_budget.as_secs(),
                    "cache alert task did not stop within drain budget; exiting anyway",
                ),
            }
        }
    };
    tokio::join!(maintenance_wait, runtime_state.wait_for_drain(drain_budget));
    Ok(())
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
