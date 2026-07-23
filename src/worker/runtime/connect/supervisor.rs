use anyhow::anyhow;
use reqwest::Client;
use std::collections::HashMap;
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;

use super::{
    WorkerConfig, WorkerRuntimeState,
    config::{managed_relay_connection_config, relay_fingerprint},
    run_relay_loop,
};
use crate::worker_admin;

#[derive(Debug)]
struct ManagedRelayTask {
    fingerprint: String,
    handle: JoinHandle<()>,
}

pub(super) async fn spawn_managed_relay_supervisor(
    config: WorkerConfig,
    client: Client,
    admin_state: worker_admin::AdminState,
    runtime_state: WorkerRuntimeState,
    relay_tasks: &mut JoinSet<()>,
) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    admin_state
        .set_relay_supervisor(worker_admin::ManagedRelaySupervisorHandle::new(tx))
        .await;
    let state = admin_state.clone();
    relay_tasks.spawn(async move {
        let mut tasks: HashMap<Uuid, ManagedRelayTask> = HashMap::new();
        let _ =
            reconcile_managed_relays(&config, &client, &state, &runtime_state, &mut tasks).await;
        loop {
            tokio::select! {
                _ = runtime_state.wait_for_shutdown() => break,
                maybe_command = rx.recv() => {
                    let Some(command) = maybe_command else {
                        break;
                    };
                    match command {
                        worker_admin::RelaySupervisorCommand::Reconcile { response } => {
                            let result = reconcile_managed_relays(
                                &config,
                                &client,
                                &state,
                                &runtime_state,
                                &mut tasks,
                            )
                            .await;
                            let _ = response.send(result);
                        }
                        worker_admin::RelaySupervisorCommand::Reconnect { relay_id, response } => {
                            let result = reconnect_managed_relay(
                                &config,
                                &client,
                                &state,
                                &runtime_state,
                                &mut tasks,
                                relay_id,
                            )
                            .await;
                            let _ = response.send(result);
                        }
                    }
                }
            }
        }
        for (_, task) in tasks {
            task.handle.abort();
        }
    });
    Ok(())
}

async fn reconcile_managed_relays(
    config: &WorkerConfig,
    client: &Client,
    admin_state: &worker_admin::AdminState,
    runtime_state: &WorkerRuntimeState,
    tasks: &mut HashMap<Uuid, ManagedRelayTask>,
) -> anyhow::Result<()> {
    let desired_relays = crate::db::list_enabled_managed_relays(&admin_state.pool).await?;
    let desired_ids = desired_relays
        .iter()
        .map(|relay| relay.relay_id)
        .collect::<Vec<_>>();

    let removed_ids = tasks
        .keys()
        .filter(|relay_id| !desired_ids.contains(relay_id))
        .copied()
        .collect::<Vec<_>>();
    for relay_id in removed_ids {
        if let Some(task) = tasks.remove(&relay_id) {
            task.handle.abort();
        }
        let mut statuses = admin_state.managed_relay_statuses.write().await;
        if let Some(status) = statuses.get_mut(&relay_id) {
            status.connected = false;
            status.last_disconnected_at = Some(chrono::Utc::now());
        }
    }

    for relay in desired_relays {
        let fingerprint = relay_fingerprint(&relay);
        let needs_restart = match tasks.get(&relay.relay_id) {
            Some(task) => task.fingerprint != fingerprint || task.handle.is_finished(),
            None => true,
        };
        if !needs_restart {
            continue;
        }

        if let Some(existing) = tasks.remove(&relay.relay_id) {
            existing.handle.abort();
        }

        let relay_config = match managed_relay_connection_config(config, admin_state, &relay).await
        {
            Ok(relay_config) => relay_config,
            Err(err) => {
                admin_state
                    .set_managed_relay_error(relay.relay_id, err.to_string())
                    .await;
                continue;
            }
        };

        let task = tokio::spawn(run_relay_loop(
            relay_config,
            config.clone(),
            client.clone(),
            Some(admin_state.clone()),
            runtime_state.clone(),
        ));
        tasks.insert(
            relay.relay_id,
            ManagedRelayTask {
                fingerprint,
                handle: task,
            },
        );
    }
    Ok(())
}

async fn reconnect_managed_relay(
    config: &WorkerConfig,
    client: &Client,
    admin_state: &worker_admin::AdminState,
    runtime_state: &WorkerRuntimeState,
    tasks: &mut HashMap<Uuid, ManagedRelayTask>,
    relay_id: Uuid,
) -> anyhow::Result<()> {
    let relay = crate::db::get_managed_relay(&admin_state.pool, relay_id)
        .await?
        .ok_or_else(|| anyhow!("relay not found"))?;
    if !relay.enabled {
        return Err(anyhow!("relay is disabled"));
    }

    if let Some(existing) = tasks.remove(&relay_id) {
        existing.handle.abort();
    }
    {
        let mut statuses = admin_state.managed_relay_statuses.write().await;
        if let Some(status) = statuses.get_mut(&relay_id) {
            status.connected = false;
            status.last_disconnected_at = Some(chrono::Utc::now());
        }
    }

    let relay_config = managed_relay_connection_config(config, admin_state, &relay).await?;
    let task = tokio::spawn(run_relay_loop(
        relay_config,
        config.clone(),
        client.clone(),
        Some(admin_state.clone()),
        runtime_state.clone(),
    ));
    tasks.insert(
        relay.relay_id,
        ManagedRelayTask {
            fingerprint: relay_fingerprint(&relay),
            handle: task,
        },
    );
    Ok(())
}

pub(super) fn require_managed_admin_state(
    admin_state: Option<&worker_admin::AdminState>,
) -> anyhow::Result<worker_admin::AdminState> {
    admin_state
        .cloned()
        .ok_or_else(|| anyhow!("managed mode requires admin state"))
}
