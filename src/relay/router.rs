use crate::{
    bridge_crypto, config::RelayConfig, ip_acl::CompiledRelayIpPolicy, relay_tls::TlsListener, tls,
};

use super::{
    public_proxy::public_router,
    state::{AppState, RelayHandle, RelayState},
    worker_bridge::worker_router,
};
use axum::{Router, body::Body, response::Response};
use futures::StreamExt;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, atomic::AtomicUsize},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, watch},
};
use tokio_rustls::TlsAcceptor;
use tracing::info;

pub async fn run(config: RelayConfig) -> anyhow::Result<()> {
    run_inner(config).await
}

pub async fn run_embedded(config: RelayConfig) -> anyhow::Result<()> {
    run_inner(config).await
}

async fn run_inner(config: RelayConfig) -> anyhow::Result<()> {
    if config.worker_heartbeat_timeout_seconds == 0 {
        anyhow::bail!("worker_heartbeat_timeout_seconds must be greater than 0");
    }
    if config.response_stream_buffer == 0 {
        anyhow::bail!("response_stream_buffer must be greater than 0");
    }
    if config.response_stream_max_bytes == 0 {
        anyhow::bail!("response_stream_max_bytes must be greater than 0");
    }
    tls::validate_relay_config(&config)?;
    tls::validate_relay_worker_config(&config)?;
    bridge_crypto::validate_settings(
        "relay",
        config.bridge_encryption_mode,
        &config.bridge_encryption_key,
    )?;
    let bind: SocketAddr = config.bind.parse()?;
    let worker_bind: SocketAddr = config.worker_bind.parse()?;
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

    info!(%bind, ?tls_mode, "relay public listening");
    info!(%worker_bind, ?worker_tls_mode, "relay worker listening");
    let public_listener = TcpListener::bind(bind).await?;
    let worker_listener = TcpListener::bind(worker_bind).await?;
    let public_shutdown_rx = shutdown_rx.clone();
    let worker_shutdown_rx = shutdown_rx.clone();
    let public_server = async move {
        let shutdown = wait_for_shutdown_signal(public_shutdown_rx);
        if let Some(acceptor) = public_acceptor {
            axum::serve(
                TlsListener::new(public_listener, acceptor),
                app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
        } else {
            axum::serve(
                public_listener,
                app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
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
        } else {
            axum::serve(
                worker_listener,
                worker_app.into_make_service_with_connect_info::<super::state::RemoteAddr>(),
            )
            .with_graceful_shutdown(shutdown)
            .await
        }
    };
    tokio::try_join!(public_server, worker_server)?;
    Ok(())
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

pub fn public_app(config: RelayConfig) -> Router {
    apps(config).0
}

pub fn public_app_with_handle(config: RelayConfig) -> (Router, RelayHandle) {
    let (public_app, _, handle) = apps(config);
    (public_app, handle)
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
