mod backoff;
mod config;
mod handshake;
mod session;
mod supervisor;
mod support;

use super::{
    SHUTDOWN_DRAIN_TIMEOUT_SECONDS, WorkerRuntimeState, ai::abort_waiting_approvals,
    build_admin_state, validate_config,
};
use crate::{config::WorkerConfig, runtime_env};
use anyhow::Context;
use reqwest::Client;
use std::time::Duration;
pub(super) use support::is_expected_relay_disconnect;
use support::shutdown_signal;
use tokio::task::JoinSet;
use tracing::info;

use self::{
    config::{
        RelayConnectionConfig, first_simple_relay_connection_config,
        simple_relay_connection_configs,
    },
    session::{connect_once, run_relay_loop},
    supervisor::{require_managed_admin_state, spawn_managed_relay_supervisor},
};

pub(super) async fn run_embedded(config: WorkerConfig) -> anyhow::Result<()> {
    validate_config(&config)?;
    if config.mode().is_shared_managed() {
        info!(
            mode = config.mode().as_str(),
            "worker storage mode selected; configured PostgreSQL is authoritative"
        );
    } else {
        let sqlite_path =
            runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
        info!(
            mode = config.mode().as_str(),
            sqlite_path = %sqlite_path.display(),
            bootstrap = "static-relay-and-upstream",
            "worker storage mode selected; standalone configuration store is not initialized yet"
        );
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .build()
        .context("failed to build upstream HTTP client")?;
    let runtime_state = WorkerRuntimeState::default();
    let admin_state = build_admin_state(&config, true).await?;
    super::abort_stale_requests_once(admin_state.as_ref()).await;
    let _stale_reconciler =
        super::spawn_stale_request_reconciler(admin_state.as_ref(), runtime_state.control.clone());
    let raw_maintenance_task = if let Some(state) = admin_state.as_ref() {
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
    let shutdown_state = runtime_state.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_state.begin_shutdown();
    });

    let mut relay_tasks = JoinSet::new();
    if config.mode().is_shared_managed() {
        let state = require_managed_admin_state(admin_state.as_ref())?;
        spawn_managed_relay_supervisor(
            config.clone(),
            client.clone(),
            state,
            runtime_state.clone(),
            &mut relay_tasks,
        )
        .await?;
    } else {
        for relay in simple_relay_connection_configs(&config)? {
            relay_tasks.spawn(run_relay_loop(
                relay,
                config.clone(),
                client.clone(),
                admin_state.clone(),
                runtime_state.clone(),
            ));
        }
    }

    runtime_state.wait_for_shutdown().await;
    if let Some(state) = admin_state.as_ref() {
        abort_waiting_approvals(state, &client).await;
    }
    relay_tasks.abort_all();
    while relay_tasks.join_next().await.is_some() {}

    if let Some(task) = raw_maintenance_task {
        let _ = task.await;
    }

    runtime_state
        .wait_for_drain(Duration::from_secs(SHUTDOWN_DRAIN_TIMEOUT_SECONDS))
        .await;
    Ok(())
}

pub(super) async fn connect_for_test(config: WorkerConfig, client: Client) -> anyhow::Result<()> {
    let relay = first_simple_relay_connection_config(&config)?;
    connect_once(&relay, config, client, None, WorkerRuntimeState::default()).await
}

pub(super) async fn connect_for_test_with_admin(
    config: WorkerConfig,
    client: Client,
) -> anyhow::Result<()> {
    let admin_state = build_admin_state(&config, false).await?;
    let relay = first_simple_relay_connection_config(&config)?;
    connect_once(
        &relay,
        config,
        client,
        admin_state,
        WorkerRuntimeState::default(),
    )
    .await
}
