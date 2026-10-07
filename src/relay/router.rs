use crate::{
    bridge_crypto, config::RelayConfig, ip_acl::CompiledRelayIpPolicy, relay_tls::TlsListener, tls,
};

use super::{
    public_proxy::public_router,
    state::{AppState, RelayHandle, RelayState},
    worker_bridge::worker_router,
};
use anyhow::Context;
use axum::{Router, body::Body, response::Response};
use futures::StreamExt;
use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, watch},
};
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};

/// Grace given to the public listener *after* shutdown is signalled. Long-lived
/// SSE / Realtime streams that outlast it are dropped so the process still exits
/// inside the orchestrator stop grace period.
const RELAY_PUBLIC_DRAIN_BUDGET: Duration = Duration::from_secs(8);
/// Extra headroom on top of the public budget for the worker bridge, so pending
/// relay -> worker reply frames get a chance to flush after the public side has
/// stopped accepting new client traffic.
const RELAY_WORKER_BRIDGE_EXTRA_DRAIN: Duration = Duration::from_secs(4);

pub async fn run(config: RelayConfig) -> anyhow::Result<()> {
    validate(&config)?;
    let bind: SocketAddr = config
        .bind
        .parse()
        .with_context(|| format!("invalid relay bind address `{}`", config.bind))?;
    let worker_bind: SocketAddr = config
        .worker_bind
        .parse()
        .with_context(|| format!("invalid relay worker bind address `{}`", config.worker_bind))?;
    let public_listener = bind_listener(bind, "relay public").await?;
    let worker_listener = bind_listener(worker_bind, "relay worker bridge").await?;
    run_inner(config, public_listener, worker_listener).await
}

pub async fn run_embedded(config: RelayConfig) -> anyhow::Result<()> {
    run(config).await
}

/// Serve on listeners the caller already reserved.
///
/// The integrated entrypoint reserves every address before anything starts, so
/// a port collision is reported before the worker connects and the derived
/// worker relay URL names the socket that will really accept the connection.
pub async fn run_with_listeners(
    config: RelayConfig,
    public_listener: std::net::TcpListener,
    worker_listener: std::net::TcpListener,
) -> anyhow::Result<()> {
    validate(&config)?;
    run_inner(
        config,
        into_tokio_listener(public_listener, "relay public")?,
        into_tokio_listener(worker_listener, "relay worker bridge")?,
    )
    .await
}

/// Reject an unusable configuration before any socket is taken.
fn validate(config: &RelayConfig) -> anyhow::Result<()> {
    if config.worker_heartbeat_timeout_seconds == 0 {
        anyhow::bail!("worker_heartbeat_timeout_seconds must be greater than 0");
    }
    if config.response_stream_buffer == 0 {
        anyhow::bail!("response_stream_buffer must be greater than 0");
    }
    if config.response_stream_max_bytes == 0 {
        anyhow::bail!("response_stream_max_bytes must be greater than 0");
    }
    if config.response_stream_backpressure_timeout_ms == 0 {
        anyhow::bail!("response_stream_backpressure_timeout_ms must be greater than 0");
    }
    tls::validate_relay_config(config)?;
    tls::validate_relay_worker_config(config)?;
    bridge_crypto::validate_settings(
        "relay",
        config.bridge_encryption_mode,
        &config.bridge_encryption_key,
    )
}

async fn bind_listener(bind: SocketAddr, listener: &'static str) -> anyhow::Result<TcpListener> {
    TcpListener::bind(bind).await.with_context(|| {
        format!("failed to bind {listener} listener to `{bind}`; set port 0 to let prompt-ferry pick a free port")
    })
}

fn into_tokio_listener(
    listener: std::net::TcpListener,
    name: &'static str,
) -> anyhow::Result<TcpListener> {
    listener.set_nonblocking(true).with_context(|| {
        format!("failed to switch the reserved {name} listener to non-blocking mode")
    })?;
    TcpListener::from_std(listener)
        .with_context(|| format!("failed to adopt the reserved {name} listener"))
}

async fn run_inner(
    config: RelayConfig,
    public_listener: TcpListener,
    worker_listener: TcpListener,
) -> anyhow::Result<()> {
    let tls_mode = config.tls_mode;
    let worker_tls_mode = config.worker_tls_mode;
    let public_config = config.clone();
    let worker_config = config.clone();
    let (app, worker_app, _) = apps(config);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        relay_shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
    let public_acceptor = if tls_mode.enabled() {
        Some(TlsAcceptor::from(tls::server_config(&public_config)?))
    } else {
        None
    };
    let worker_acceptor = if worker_tls_mode.enabled() {
        Some(TlsAcceptor::from(tls::worker_server_config(
            &worker_config,
        )?))
    } else {
        None
    };

    // The listeners are already bound here, so this reports the address that
    // is really serving rather than the configured one.
    let public_addr = public_listener.local_addr()?;
    let worker_addr = worker_listener.local_addr()?;
    info!(%public_addr, ?tls_mode, "relay public listening");
    info!(%worker_addr, ?worker_tls_mode, "relay worker listening");
    let public_shutdown_rx = shutdown_rx.clone();
    let worker_shutdown_rx = shutdown_rx.clone();
    // The public side aborts first so new client traffic stops getting
    // accepted; the worker bridge gets a brief extra budget to flush any
    // pending relay -> worker reply frames before it is also forced.
    let public_server = async move {
        let shutdown = wait_for_shutdown_signal(public_shutdown_rx);
        if let Some(acceptor) = public_acceptor {
            axum::serve(
                TlsListener::new(public_listener, acceptor),
                app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(anyhow::Error::from)
        } else {
            axum::serve(
                public_listener,
                app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(anyhow::Error::from)
        }
    };
    let worker_server = async move {
        let shutdown = wait_for_shutdown_signal(worker_shutdown_rx);
        if let Some(acceptor) = worker_acceptor {
            axum::serve(
                TlsListener::new(worker_listener, acceptor),
                worker_app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(anyhow::Error::from)
        } else {
            axum::serve(
                worker_listener,
                worker_app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(anyhow::Error::from)
        }
    };
    // Both listeners run concurrently for the whole life of the process. The
    // budgets below do not start counting until shutdown has been signalled,
    // so a healthy relay is never interrupted by them.
    tokio::try_join!(
        drain_within(
            "public",
            public_server,
            shutdown_rx.clone(),
            RELAY_PUBLIC_DRAIN_BUDGET,
        ),
        drain_within(
            "worker-bridge",
            worker_server,
            shutdown_rx.clone(),
            RELAY_PUBLIC_DRAIN_BUDGET + RELAY_WORKER_BRIDGE_EXTRA_DRAIN,
        ),
    )?;
    Ok(())
}

/// Drive one listener for the lifetime of the process, capping only the
/// post-shutdown drain. `grace` starts when the shutdown signal fires; dropping
/// the listener future on expiry aborts whatever is still in flight.
async fn drain_within<F>(
    listener: &'static str,
    serve: F,
    shutdown_rx: watch::Receiver<bool>,
    grace: Duration,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    tokio::select! {
        result = serve => result,
        _ = drain_deadline(shutdown_rx, grace) => {
            warn!(
                listener,
                grace_seconds = grace.as_secs(),
                "relay listener did not drain within grace; forcing abort"
            );
            Ok(())
        }
    }
}

/// Resolves `grace` after shutdown is signalled, and never before — while the
/// relay serves normally this stays parked on the watch channel.
async fn drain_deadline(shutdown_rx: watch::Receiver<bool>, grace: Duration) {
    wait_for_shutdown_signal(shutdown_rx).await;
    tokio::time::sleep(grace).await;
}

async fn wait_for_shutdown_signal(mut shutdown_rx: watch::Receiver<bool>) {
    if *shutdown_rx.borrow() {
        return;
    }
    let _ = shutdown_rx.changed().await;
}

async fn relay_shutdown_signal() {
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

pub fn apps(config: RelayConfig) -> (Router, Router, RelayHandle) {
    let inner = relay_state();
    let state = AppState {
        config,
        inner: inner.clone(),
    };
    let public_app = public_router(state.clone());
    let worker_app = worker_router(state);
    (public_app, worker_app, RelayHandle { inner })
}

fn relay_state() -> Arc<RelayState> {
    Arc::new(RelayState {
        workers: Mutex::new(HashMap::new()),
        worker_loads: Mutex::new(HashMap::new()),
        pending: Mutex::new(HashMap::new()),
        pending_mcp: Mutex::new(HashMap::new()),
        pending_realtime_sessions: Mutex::new(HashMap::new()),
        routes: Mutex::new(HashMap::new()),
        relay_ip_policy: Mutex::new(CompiledRelayIpPolicy::default()),
        config_version: Mutex::new(None),
        next_worker_id: AtomicUsize::new(1),
    })
}

pub(crate) async fn drain_body_then(body: Body, response: Response) -> Response {
    let mut stream = body.into_data_stream();
    while let Some(next) = stream.next().await {
        if next.is_err() {
            break;
        }
    }
    response
}
