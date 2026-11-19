//! The gated database node runtime.
//!
//! One node owns one listener. It serves the public AI surface and the
//! worker admin API from that single listener, and it executes the requests
//! locally: the relay side of the node and the executor side are joined by
//! the in-memory bridge link, so a request travels exactly the path it
//! travels between a relay and a remote worker.
//!
//! The runtime is the default startup: a no-subcommand invocation loads the
//! node configuration and calls [`run_node`]. The legacy relay/worker startup
//! modules still compile until their source cleanup phase.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use reqwest::Client;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    bridge::link::{InMemoryLink, in_memory_link},
    config::{NODE_ENDPOINT_PATH, NodeConfig, WorkerConfig},
    db,
    db::gate::{GateState, RuntimeIdentity},
    relay::{attach_in_memory_worker, node_apps},
    relay_tls::TlsListener,
    tls,
    worker_admin::{self, AdminState},
};

use super::{
    WorkerRuntimeState, WorkerShutdown,
    bootstrap::build_admin_state_from_pools,
    lifecycle::{RuntimeControl, join_owned_tasks, stop_accepted_tasks},
    registry,
};

pub(crate) mod identity;
pub(crate) mod peer;

use self::identity::NodeId;
use self::peer::{
    DbPeerContext, NodeIdentity, NodeRole, PeerLinkContext, PeerRoleContext, PeerTarget,
    peer_challenge, peer_upgrade, run_peer_dialer,
};

/// The coordinated store type of the shared Valkey scheduler.
type CoordinatedStore = scheduler::ValkeyCoordinatedStateStore;

/// The key the local relay link is registered under.
///
/// The node has no remote relay, so the snapshot sender is registered under
/// a local name that no remote configuration can collide with.
const NODE_RELAY_KEY: &str = "node-local";

/// Upper bound for the node gate claim and the migration it owns.
const GATE_PREPARE_TIMEOUT: Duration = Duration::from_secs(300);

/// How often the gate watcher reads the gate metadata.
///
/// The watcher owns a dedicated control connection that holds no shared
/// application barrier, so it can never delay an application session.
const GATE_WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// Upper bound for one startup reconciliation of the stale request records.
const STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Allowance for the owned background tasks to stop.
const BACKGROUND_STOP_ALLOWANCE: Duration = Duration::from_secs(10);

/// Upper bound for the last wait of the accepted work.
const FINAL_DRAIN_WAIT: Duration = Duration::from_secs(10);

/// Upper bound for the listener to finish its graceful shutdown.
const LISTENER_STOP_ALLOWANCE: Duration = Duration::from_secs(10);

/// Upper bound for closing the gated pools.
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// A database node with a persistent identity.
///
/// The identity is the node id of this node; it is persisted by the caller
/// and stays the same across restarts, so the registry row of the node keeps
/// its meaning. The node id is not the group id: the group is derived from
/// the database node later.
pub struct NodeDbRuntime {
    config: NodeConfig,
    node_id: Uuid,
    stop_old_ack: bool,
}

impl NodeDbRuntime {
    pub fn new(config: NodeConfig, node_id: Uuid, stop_old_ack: bool) -> Self {
        Self {
            config,
            node_id,
            stop_old_ack,
        }
    }

    /// The node id of this runtime.
    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    /// Serve the node until it is asked to stop.
    pub async fn serve(self) -> anyhow::Result<()> {
        self.config
            .validate()
            .map_err(|err| anyhow!("invalid node configuration: {err}"))?;
        let database_url = self.config.database_url.trim().to_string();
        if database_url.is_empty() {
            return Err(anyhow!(
                "the node runtime requires a database URL; a database-less node is not \
                 implemented yet"
            ));
        }
        let worker_config = self.config.worker_config(&self.node_id.to_string())?;
        let relay_config = self.config.relay_config();
        let manager = self.config.secret_manager()?;
        // The peer links of the node prove the shared master secret, so the
        // node keeps a second handle of the same secret.
        let peer_manager = manager.clone();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(worker_config.connect_timeout_seconds))
            .build()
            .context("failed to build upstream HTTP client")?;

        // The gate owns the schema: it runs before any application pool is
        // opened, so no application session can exist while the migration
        // runs.
        let identity =
            RuntimeIdentity::current().context("failed to resolve the runtime identity")?;
        let owner = Uuid::new_v4();
        let gate = db::gate::NodeGate::prepare_database(
            &database_url,
            identity,
            owner,
            self.stop_old_ack,
            GATE_PREPARE_TIMEOUT,
        )
        .await
        .context("failed to prepare the node database")?;
        info!(
            runtime_version = %gate.identity.version,
            protocol = gate.identity.protocol,
            generation = gate.generation,
            "node database prepared"
        );

        // Every application session holds the shared barrier of the gate, so
        // a later upgrade waits for these connections to be gone.
        let pool = db::gate::connect_node_pool(&database_url, 8, gate)
            .await
            .context("failed to open the gated node pool")?;
        let lease_pool = match db::gate::connect_node_pool(&database_url, 8, gate).await {
            Ok(lease_pool) => lease_pool,
            Err(err) => {
                close_pool(&pool).await;
                return Err(err.context("failed to open the gated node lease pool"));
            }
        };
        let mut admin_state = match build_admin_state_from_pools(
            &worker_config,
            pool.clone(),
            lease_pool.clone(),
            manager,
            Some(self.config.valkey_url.as_str()).filter(|url| !url.trim().is_empty()),
        )
        .await
        {
            Ok(state) => state,
            Err(err) => {
                close_pool(&lease_pool).await;
                close_pool(&pool).await;
                return Err(err);
            }
        };

        let master = *self
            .config
            .master_secret
            .as_ref()
            .expect("validated master secret")
            .as_bytes();
        admin_state.node_setup = Some(Arc::new(crate::worker_admin_state::NodeSetupAuth::new(
            master,
        )));
        admin_state.node_status = Some(Arc::new(std::sync::Mutex::new(
            crate::worker_admin_state::NodeStatusState {
                own: crate::worker_admin_types::NodeStatusResponse {
                    node_id: self.node_id,
                    group_id: self.node_id,
                    has_database: true,
                    runtime_version: env!("CARGO_PKG_VERSION").to_owned(),
                    protocol_version: crate::db::gate::NODE_PROTOCOL_VERSION.to_string(),
                    peers: Vec::new(),
                },
                peers: std::collections::HashMap::new(),
            },
        )));

        let runtime_state = WorkerRuntimeState::default();
        let worker_shutdown = WorkerShutdown::new();
        runtime_state
            .control
            .attach_shutdown_broadcast(worker_shutdown.clone());
        let shared_valkey = admin_state.shared_valkey.clone();
        let coordinated_store = match coordinated_store(&shared_valkey).await {
            Ok(store) => store,
            Err(err) => {
                close_pool(&lease_pool).await;
                close_pool(&pool).await;
                return Err(err);
            }
        };
        let coordination = if shared_valkey.is_configured() {
            db::CoordinationSnapshot::shared(shared_valkey.revision())
        } else {
            db::CoordinationSnapshot::bootstrap(shared_valkey.revision())
        };
        // The admission is the gate of the whole node: the identity is
        // claimed before the listener and the schedulers start.
        let admitted_at = match registry::admit_worker(
            &admin_state.pool,
            &worker_config,
            &runtime_state.control,
            coordination,
        )
        .await
        {
            Ok(admitted_at) => admitted_at,
            Err(err) => {
                close_pool(&lease_pool).await;
                close_pool(&pool).await;
                return Err(err);
            }
        };
        let (registry_stop, registry_task, lease_watchdog) = spawn_registry_tasks(
            &admin_state,
            &worker_config,
            &runtime_state.control,
            coordination,
            admitted_at,
        );

        // The stale request reconciliation runs before the listener starts, so
        // the node does not serve while it is still reconciling.
        if tokio::time::timeout(
            STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT,
            super::abort_stale_requests_once(Some(&admin_state)),
        )
        .await
        .is_err()
        {
            warn!(
                timeout_seconds = STALE_REQUEST_RECONCILE_STARTUP_TIMEOUT.as_secs(),
                "the startup request reconciliation did not finish in time; continuing"
            );
        }

        let mut background_tasks = spawn_background_tasks(
            &admin_state,
            &worker_config,
            &shared_valkey,
            &client,
            &runtime_state.control,
            coordinated_store.clone(),
            &database_url,
        );

        // The local link joins the relay side of this node with its executor
        // side: the request travels the same typed bridge messages it travels
        // over a websocket.
        let InMemoryLink {
            relay_request_tx,
            executor_request_rx,
            executor_response_tx,
            relay_response_rx,
        } = in_memory_link();
        // The heartbeat of the peer links uses the same budget as the
        // heartbeat of a remote worker.
        let peer_heartbeat_timeout =
            Duration::from_secs(relay_config.worker_heartbeat_timeout_seconds.max(1));
        let (app, relay_state) = node_apps(relay_config, worker_admin::router(admin_state.clone()));

        // The peer endpoint of the node is part of its single listener: an
        // edge node may dial the database node as well as the other way
        // round.
        let peer_context = PeerLinkContext::new(
            NodeIdentity {
                node_id: NodeId::from_uuid(self.node_id),
                role: NodeRole::Database,
                manager: peer_manager,
            },
            PeerRoleContext::Database(Arc::new(DbPeerContext {
                config: worker_config.clone(),
                admin_state: admin_state.clone(),
                client: client.clone(),
                runtime_state: runtime_state.clone(),
                heartbeat_timeout: peer_heartbeat_timeout,
                node_id: NodeId::from_uuid(self.node_id),
            })),
            peer_heartbeat_timeout,
        );
        let upgrade_context = peer_context.clone();
        let challenge_context = peer_context.clone();
        let app = app
            .route(
                NODE_ENDPOINT_PATH,
                axum::routing::get(move |request: axum::extract::Request| {
                    let context = upgrade_context.clone();
                    async move { peer_upgrade(context, request).await }
                }),
            )
            .route(
                peer::CHALLENGE_PATH,
                axum::routing::get(move || {
                    let context = challenge_context.clone();
                    async move { peer_challenge(context).await }
                }),
            );
        let worker_id = relay_state.allocate_worker_id();
        let mut link_task = match attach_in_memory_worker(
            relay_state.clone(),
            worker_id,
            relay_request_tx,
            relay_response_rx,
        )
        .await
        {
            Some(task) => Some(task),
            None => {
                stop_started_tasks(&mut background_tasks, None, None, None).await;
                stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
                release_admission(&admin_state, &worker_config, &runtime_state.control).await;
                close_pool(&lease_pool).await;
                close_pool(&pool).await;
                return Err(anyhow!("the local executor was refused as a relay worker"));
            }
        };
        let executor = super::ExecutorPump::new(
            worker_config.clone(),
            Some(admin_state.clone()),
            executor_response_tx.clone(),
            client.clone(),
            runtime_state.clone(),
            None,
        )
        .with_node_attribution(crate::db::NodeAttribution::local(self.node_id));
        let mut pump_task = Some(tokio::spawn(async move {
            executor.run(executor_request_rx).await
        }));

        // The snapshot sender is registered and the snapshot is published
        // before the listener accepts business traffic. The routes of the
        // node become usable once the snapshot reached the relay side; until
        // then the readiness probe stays red and no request is faked as
        // served.
        let (admin_tx, mut admin_rx) =
            tokio::sync::mpsc::unbounded_channel::<crate::protocol::BridgeMessage>();
        let admin_out_tx = executor_response_tx.clone();
        let mut admin_forward_task = Some(tokio::spawn(async move {
            while let Some(message) = admin_rx.recv().await {
                if admin_out_tx.send(message).await.is_err() {
                    break;
                }
            }
        }));
        // The last owner of the response channel is dropped here: the
        // executor and the snapshot forwarder hold the remaining halves, so
        // the relay side sees the ordinary end of the worker link when they
        // stop.
        drop(executor_response_tx);
        worker_admin::set_bridge_sender(&admin_state, NODE_RELAY_KEY, Some(admin_tx)).await;
        if let Err(err) = worker_admin::publish_snapshot(&admin_state).await {
            warn!(error = %err, "failed to publish the initial node configuration snapshot");
        }

        let listener = match tokio::net::TcpListener::bind(self.config.bind).await {
            Ok(listener) => listener,
            Err(err) => {
                stop_started_tasks(
                    &mut background_tasks,
                    admin_forward_task.take(),
                    pump_task.take(),
                    link_task.take(),
                )
                .await;
                stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
                release_admission(&admin_state, &worker_config, &runtime_state.control).await;
                close_pool(&lease_pool).await;
                close_pool(&pool).await;
                return Err(err).context("failed to bind the node listener");
            }
        };
        let bound_addr = listener
            .local_addr()
            .context("failed to read the address of the node listener")?;
        info!(%bound_addr, "node listening");
        let tls_acceptor = match self.config.tls_mode {
            crate::config::NodeTlsMode::Off => None,
            _ => match tls::node_server_config(&self.config) {
                Ok(server_config) => Some(TlsAcceptor::from(server_config)),
                Err(err) => {
                    stop_started_tasks(
                        &mut background_tasks,
                        admin_forward_task.take(),
                        pump_task.take(),
                        link_task.take(),
                    )
                    .await;
                    stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
                    release_admission(&admin_state, &worker_config, &runtime_state.control).await;
                    close_pool(&lease_pool).await;
                    close_pool(&pool).await;
                    return Err(err);
                }
            },
        };
        let serve_control = runtime_state.control.clone();
        let serve_task = tokio::spawn(async move {
            let shutdown = {
                let control = serve_control;
                async move { control.wait_for_shutdown().await }
            };
            let serve = async {
                if let Some(acceptor) = tls_acceptor {
                    axum::serve(
                        TlsListener::new(listener, acceptor),
                        app.into_make_service_with_connect_info::<crate::relay::RemoteAddr>(),
                    )
                    .with_graceful_shutdown(shutdown)
                    .await
                    .map_err(anyhow::Error::from)
                } else {
                    axum::serve(
                        listener,
                        app.into_make_service_with_connect_info::<crate::relay::RemoteAddr>(),
                    )
                    .with_graceful_shutdown(shutdown)
                    .await
                    .map_err(anyhow::Error::from)
                }
            };
            serve.await
        });

        // The node dials its peers as well: either side may start a link, and
        // the deduplication of the directory keeps one link per peer.
        let (peer_shutdown_tx, peer_shutdown_rx) = tokio::sync::watch::channel(false);
        let mut peer_tasks: Vec<JoinHandle<()>> = Vec::new();
        for peer_url in &self.config.peer_urls {
            let target = match PeerTarget::parse(peer_url) {
                Ok(target) => target,
                Err(err) => {
                    stop_started_tasks(
                        &mut background_tasks,
                        admin_forward_task.take(),
                        pump_task.take(),
                        link_task.take(),
                    )
                    .await;
                    stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
                    release_admission(&admin_state, &worker_config, &runtime_state.control).await;
                    close_pool(&lease_pool).await;
                    close_pool(&pool).await;
                    return Err(err);
                }
            };
            let peer_tls = match target.secure {
                true => match tls::node_client_config(&self.config) {
                    Ok(config) => Some(config),
                    Err(err) => {
                        stop_started_tasks(
                            &mut background_tasks,
                            admin_forward_task.take(),
                            pump_task.take(),
                            link_task.take(),
                        )
                        .await;
                        stop_registry_tasks(registry_stop, registry_task, lease_watchdog).await;
                        release_admission(&admin_state, &worker_config, &runtime_state.control)
                            .await;
                        close_pool(&lease_pool).await;
                        close_pool(&pool).await;
                        return Err(err);
                    }
                },
                false => None,
            };
            let dial_context = peer_context.clone();
            let dial_shutdown = peer_shutdown_rx.clone();
            peer_tasks.push(tokio::spawn(async move {
                run_peer_dialer(dial_context, target, peer_tls, dial_shutdown).await;
            }));
        }

        // The gate watcher owns a dedicated control connection that holds no
        // shared application barrier. A node that may no longer serve stops
        // instead of keeping the listener open next to an incompatible
        // runtime.
        let gate_watch_task = spawn_gate_watcher(
            database_url.clone(),
            identity,
            runtime_state.control.clone(),
        );

        // The OS signal uses the same internal shutdown path as an internal
        // failure, so both stop the listener, the executor and the
        // schedulers.
        let signal_state = runtime_state.clone();
        let signal_task = tokio::spawn(async move {
            shutdown_signal().await;
            signal_state.begin_shutdown();
        });

        let outcome = shutdown_node(NodeShutdown {
            runtime_state: runtime_state.clone(),
            admin_state: admin_state.clone(),
            worker_config: worker_config.clone(),
            serve_task,
            pump_task: pump_task.expect("the executor task is owned"),
            link_task: link_task.expect("the link task is owned"),
            admin_forward_task: admin_forward_task.expect("the snapshot forwarder is owned"),
            peer_tasks,
            peer_shutdown_tx,
            peer_context: peer_context.clone(),
            background_tasks,
            registry_stop,
            registry_task,
            lease_watchdog,
        })
        .await;

        signal_task.abort();
        let _ = signal_task.await;
        gate_watch_task.abort();
        let _ = gate_watch_task.await;
        worker_shutdown.trigger();
        outcome
    }
}

/// The owned parts of a running node that the shutdown path stops.
struct NodeShutdown {
    runtime_state: WorkerRuntimeState,
    admin_state: AdminState,
    worker_config: WorkerConfig,
    serve_task: JoinHandle<anyhow::Result<()>>,
    pump_task: JoinHandle<()>,
    link_task: JoinHandle<()>,
    admin_forward_task: JoinHandle<()>,
    peer_tasks: Vec<JoinHandle<()>>,
    peer_shutdown_tx: tokio::sync::watch::Sender<bool>,
    peer_context: Arc<PeerLinkContext>,
    background_tasks: Vec<JoinHandle<()>>,
    registry_stop: Option<tokio::sync::watch::Sender<bool>>,
    registry_task: Option<JoinHandle<()>>,
    lease_watchdog: Option<JoinHandle<()>>,
}

/// Stop the tasks a failed startup already spawned.
///
/// Every handle is aborted and joined, so a startup error never leaves a
/// detached task behind that could still write to the database.
async fn stop_started_tasks(
    background_tasks: &mut Vec<JoinHandle<()>>,
    admin_forward_task: Option<JoinHandle<()>>,
    pump_task: Option<JoinHandle<()>>,
    link_task: Option<JoinHandle<()>>,
) {
    if let Some(task) = admin_forward_task {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = pump_task {
        task.abort();
        let _ = task.await;
    }
    if let Some(mut task) = link_task
        && tokio::time::timeout(LISTENER_STOP_ALLOWANCE, &mut task)
            .await
            .is_err()
    {
        task.abort();
        let _ = task.await;
    }
    for task in background_tasks.drain(..) {
        task.abort();
        let _ = task.await;
    }
}

async fn stop_peer_dialers_on_shutdown(
    control: &RuntimeControl,
    shutdown: &tokio::sync::watch::Sender<bool>,
) {
    control.wait_for_shutdown().await;
    shutdown.send_replace(true);
}

/// Stop the node and release everything it owns.
///
/// The order is fixed: the listener stops accepting, the accepted work
/// drains inside the existing shutdown budget while the response link is
/// still alive, the background schedulers stop, the link and the executor
/// stop, the registry row is released and only then are the gated pools
/// closed. Closing a pool before the registry row is released would break
/// the release itself.
async fn shutdown_node(node: NodeShutdown) -> anyhow::Result<()> {
    let runtime_state = node.runtime_state;
    let control = runtime_state.control.clone();
    stop_peer_dialers_on_shutdown(&control, &node.peer_shutdown_tx).await;
    let fail_closed = control.is_fail_closed();
    let drain_budget = node_drain_budget(fail_closed, node.worker_config.shutdown_drain_seconds);
    info!(
        budget_seconds = drain_budget.as_secs(),
        fail_closed, "node shutdown admission closed; draining accepted request rounds"
    );
    // The response link is still alive here: a request that is already
    // running can still deliver its response through it.
    let remaining = tokio::select! {
        remaining = control.wait_for_drain(drain_budget) => remaining,
        _ = control.wait_for_fail_closed() => usize::MAX,
    };
    let fail_closed = fail_closed || control.is_fail_closed();

    // The maintenance scheduling stops with the node.
    let mut background_tasks = node.background_tasks;
    join_owned_tasks(&mut background_tasks, BACKGROUND_STOP_ALLOWANCE, &control).await;

    if must_cancel_accepted_work(fail_closed, remaining) {
        warn!(
            remaining,
            budget_seconds = drain_budget.as_secs(),
            fail_closed,
            "node shutdown drain ended; cancelling the remaining requests"
        );
        super::lifecycle::cancel_in_flight_requests(&runtime_state).await;
        stop_accepted_tasks(&runtime_state, Duration::ZERO).await;
    }
    stop_accepted_tasks(&runtime_state, FINAL_DRAIN_WAIT).await;

    // The listener stops accepting and finishes its in-flight requests.
    let mut serve_task = node.serve_task;
    if tokio::time::timeout(LISTENER_STOP_ALLOWANCE, &mut serve_task)
        .await
        .is_err()
    {
        warn!(
            allowance_seconds = LISTENER_STOP_ALLOWANCE.as_secs(),
            "the node listener did not finish its graceful shutdown in time; aborting it"
        );
        serve_task.abort();
        let _ = serve_task.await;
    }

    // The executor and the snapshot forwarder stop, and the response link is
    // closed: the relay side performs the ordinary worker cleanup.
    node.admin_forward_task.abort();
    let _ = node.admin_forward_task.await;
    node.pump_task.abort();
    let _ = node.pump_task.await;
    let mut link_task = node.link_task;
    if tokio::time::timeout(LISTENER_STOP_ALLOWANCE, &mut link_task)
        .await
        .is_err()
    {
        warn!("the node link did not stop in time; aborting it");
        link_task.abort();
        let _ = link_task.await;
    }
    worker_admin::set_bridge_sender(&node.admin_state, NODE_RELAY_KEY, None).await;

    // The peer links are closed and joined before the pools are closed: a
    // link that is still running would use the pools of the node. The
    // accepted sessions are closed first, and the dialers afterwards.
    node.peer_context.stop_sessions().await;
    for peer_task in node.peer_tasks {
        peer_task.abort();
        let _ = peer_task.await;
    }

    // The registry row is released before the pools are closed.
    stop_registry_tasks(node.registry_stop, node.registry_task, node.lease_watchdog).await;
    release_admission(&node.admin_state, &node.worker_config, &control).await;

    close_pool(&node.admin_state.lease_pool).await;
    close_pool(&node.admin_state.pool).await;
    Ok(())
}

/// The registry tasks of a node: the stop signal, the heartbeat and the
/// lease watchdog.
type RegistryTasks = (
    Option<tokio::sync::watch::Sender<bool>>,
    Option<JoinHandle<()>>,
    Option<JoinHandle<()>>,
);

/// Start the registry heartbeat and the lease watchdog of the node.
#[allow(clippy::too_many_arguments)]
fn spawn_registry_tasks(
    admin_state: &AdminState,
    worker_config: &WorkerConfig,
    control: &RuntimeControl,
    coordination: db::CoordinationSnapshot,
    admitted_at: std::time::Instant,
) -> RegistryTasks {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let tracker = registry::LeaseTracker::new();
    let lease_watchdog =
        registry::spawn_lease_watchdog(control, admitted_at, tracker.clone(), stop_rx.clone());
    let registry_task = registry::spawn_worker_registry(
        admin_state.pool.clone(),
        worker_config,
        control,
        coordination.revision,
        coordination.mode,
        admitted_at,
        tracker,
        stop_rx,
    );
    (Some(stop_tx), Some(registry_task), Some(lease_watchdog))
}

/// Start the background schedulers of the node.
///
/// They are the existing background jobs of a worker, so a node and a worker
/// share the same global exclusion units and the same coordinated schedule.
fn spawn_background_tasks(
    admin_state: &AdminState,
    worker_config: &WorkerConfig,
    shared_valkey: &db::SharedValkeyConfig,
    client: &Client,
    control: &RuntimeControl,
    coordinated_store: Option<CoordinatedStore>,
    database_url: &str,
) -> Vec<JoinHandle<()>> {
    let raw_maintenance = super::raw_maintenance::spawn(
        shared_valkey,
        admin_state.pool.clone(),
        admin_state.usage_retention.clone(),
        control.clone(),
        coordinated_store.clone(),
    );
    let cache_alert = super::cache_alert_monitor::spawn_cache_alert_monitor(
        shared_valkey,
        admin_state.pool.clone(),
        admin_state.cache_alert.clone(),
        client.clone(),
        control.clone(),
        coordinated_store.clone(),
    );
    let chatgpt_quota = admin_state.chatgpt_quota_service.clone().map(|service| {
        super::chatgpt_quota_collector::spawn(
            shared_valkey,
            service,
            control.clone(),
            coordinated_store.clone(),
        )
    });
    let quota_observation = super::quota_observation_collector::spawn(
        shared_valkey,
        admin_state.pool.clone(),
        admin_state.token_plan_quota.clone(),
        control.clone(),
        coordinated_store.clone(),
    );
    let mcp_catalog_refresh = super::mcp_catalog_refresh::spawn(
        worker_config,
        admin_state.mcp_catalog_service.clone(),
        control.clone(),
    );
    let config_refresh = super::config_refresh::spawn_node_config_refresh(
        admin_state.clone(),
        control.clone(),
        database_url.to_string(),
    );
    let stale_reconciler =
        super::spawn_stale_request_reconciler(Some(admin_state), control.clone());
    [
        Some(raw_maintenance),
        Some(cache_alert),
        chatgpt_quota,
        Some(quota_observation),
        Some(mcp_catalog_refresh),
        Some(config_refresh),
        stale_reconciler,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Watch the node gate and stop the node when it may no longer serve.
///
/// The watcher owns a dedicated control connection. That connection holds no
/// shared application barrier, so reading the gate can never delay an
/// application session or hold back an upgrade claim.
fn spawn_gate_watcher(
    database_url: String,
    identity: RuntimeIdentity,
    control: RuntimeControl,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut gate = match db::gate::GateControl::connect(&database_url).await {
            Ok(gate) => gate,
            Err(err) => {
                warn!(error = %err, "failed to open the node gate control connection");
                control.begin_fail_closed_shutdown();
                return;
            }
        };
        let mut interval = tokio::time::interval(GATE_WATCH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match gate.read().await {
                Ok(snapshot) => {
                    if snapshot.state != GateState::Ready || snapshot.identity != identity {
                        warn!(
                            state = ?snapshot.state,
                            runtime_version = %snapshot.identity.version,
                            protocol = snapshot.identity.protocol,
                            "the node gate no longer admits this runtime; stopping the node"
                        );
                        control.begin_fail_closed_shutdown();
                        return;
                    }
                }
                Err(err) => {
                    warn!(error = %err, "failed to read the node gate; stopping the node");
                    control.begin_fail_closed_shutdown();
                    return;
                }
            }
        }
    })
}

/// Connect the coordinated store of the shared Valkey, when one is configured.
async fn coordinated_store(
    shared_valkey: &db::SharedValkeyConfig,
) -> anyhow::Result<Option<CoordinatedStore>> {
    if !shared_valkey.is_configured() {
        return Ok(None);
    }
    let store = super::background_job::connect_coordinated_store(shared_valkey.url_or_empty())
        .await
        .ok_or_else(|| anyhow!("the configured shared Valkey coordination is not available"))?;
    Ok(Some(store))
}

/// Stop the registry tasks of the node.
async fn stop_registry_tasks(
    registry_stop: Option<tokio::sync::watch::Sender<bool>>,
    registry_task: Option<JoinHandle<()>>,
    lease_watchdog: Option<JoinHandle<()>>,
) {
    if let Some(stop_tx) = registry_stop {
        let _ = stop_tx.send(true);
    }
    if let Some(task) = registry_task
        && let Err(err) = task.await
    {
        warn!(error = %err, "node registry task join failed during shutdown");
    }
    if let Some(task) = lease_watchdog
        && let Err(err) = task.await
    {
        warn!(error = %err, "node lease watchdog join failed during shutdown");
    }
}

/// Release the registry admission of the node.
async fn release_admission(
    admin_state: &AdminState,
    worker_config: &WorkerConfig,
    control: &RuntimeControl,
) {
    registry::release_worker(
        &admin_state.pool,
        &worker_config.resolved_worker_identity(),
        control.worker_instance_id(),
    )
    .await;
}

/// The drain budget of a node shutdown.
///
/// A node that lost its right to serve does not drain: the accepted work is
/// cancelled instead, because another runtime may already own the identity.
fn node_drain_budget(fail_closed: bool, configured_seconds: u64) -> Duration {
    RuntimeControl::shutdown_drain_budget(fail_closed, configured_seconds)
}

/// Whether the accepted work must be cancelled instead of drained.
fn must_cancel_accepted_work(fail_closed: bool, remaining: usize) -> bool {
    fail_closed || remaining > 0
}

/// Close a gated pool inside a bounded budget.
///
/// A pool that does not close in time is reported: the barrier of its
/// sessions is then still held, so an upgrade waits for it instead of
/// migrating underneath a live session.
async fn close_pool(pool: &sqlx::PgPool) {
    if tokio::time::timeout(POOL_CLOSE_TIMEOUT, pool.close())
        .await
        .is_err()
    {
        warn!(
            timeout_seconds = POOL_CLOSE_TIMEOUT.as_secs(),
            "a gated node pool did not close within the budget"
        );
    }
}

/// Wait for the operating system shutdown signal.
///
/// The node uses the same internal shutdown path for an OS signal and for
/// an internal failure, so both stop the listener, the executor and the
/// schedulers in the same order.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Serve a node until it is asked to stop.
///
/// This is the entrypoint of the node runtime: the node owns one listener,
/// one persistent identity and, when it is the database node of its star,
/// the peers that belong to it. The database URL decides the role: a node
/// without a database is an edge node, and a node with a database is the
/// database node.
pub async fn run_node(config: NodeConfig, stop_old_ack: bool) -> anyhow::Result<()> {
    config
        .validate()
        .map_err(|err| anyhow!("invalid node configuration: {err}"))?;
    let node_id_path =
        identity::node_id_path(None).context("failed to resolve the location of the node id")?;
    let node_id = identity::load_or_create_node_id(&node_id_path)
        .context("failed to load the identity of the node")?;
    info!(node_id = %node_id, "node identity loaded");
    if config.database_url.trim().is_empty() {
        return peer::run_edge_node(config, node_id).await;
    }
    serve_node(config, node_id.as_uuid(), stop_old_ack).await
}

/// Serve a database node until it is asked to stop.
///
/// This is the public entrypoint of the node runtime: the caller owns the
/// persistent node id and the maintenance acknowledgement, and the runtime
/// owns everything else.
pub async fn serve_node(
    config: NodeConfig,
    node_id: Uuid,
    stop_old_ack: bool,
) -> anyhow::Result<()> {
    NodeDbRuntime::new(config, node_id, stop_old_ack)
        .serve()
        .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{must_cancel_accepted_work, node_drain_budget};

    #[tokio::test]
    async fn peer_dialers_are_stopped_only_when_shutdown_begins() {
        let control = super::RuntimeControl::new();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let task_control = control.clone();
        let task = tokio::spawn(async move {
            super::stop_peer_dialers_on_shutdown(&task_control, &tx).await;
        });
        tokio::task::yield_now().await;
        assert!(!*rx.borrow());
        assert!(!task.is_finished());
        control.begin_shutdown();
        task.await.expect("shutdown task completes");
        assert!(*rx.borrow());
    }

    #[test]
    fn a_node_drains_inside_the_configured_budget() {
        assert_eq!(node_drain_budget(false, 120), Duration::from_secs(120));
        assert_eq!(
            node_drain_budget(false, 0),
            Duration::from_secs(1),
            "a configured budget of zero still allows one second"
        );
    }

    #[test]
    fn a_node_that_lost_its_right_to_serve_cancels_instead_of_draining() {
        assert_eq!(node_drain_budget(true, 120), Duration::from_secs(0));
        assert!(must_cancel_accepted_work(true, 0));
        assert!(must_cancel_accepted_work(false, 1));
        assert!(!must_cancel_accepted_work(false, 0));
    }
}
