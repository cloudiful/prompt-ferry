//! The database-less edge node.
//!
//! An edge node has no database, no registry row and no background
//! scheduler. It owns one listener, serves the public API itself and
//! forwards the business traffic of its clients to the database node of its
//! star over the authenticated peer link. Everything it does not serve
//! itself — the administration pages, the API and the assets — is forwarded
//! to the database node unchanged.

use std::sync::Arc;

use anyhow::anyhow;
use axum::routing::get;
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};

use crate::config::NodeConfig;
use crate::relay::node_edge_apps;
use crate::relay_tls::TlsListener;
use crate::tls;

use super::dial::{PeerTarget, run_peer_dialer};
use super::topology::NodeRole;
use super::{NodeIdentity, PeerLinkContext, PeerRoleContext, peer_challenge, peer_upgrade};
use crate::worker::runtime::node::identity::NodeId;

/// Serve a database-less node until it is asked to stop.
///
/// The node may dial every peer URL of its configuration: a node that has no
/// route of its own reaches the database node over one of them, and a node
/// that has one uses the others for the traffic of its own peers. With no
/// peer URL the listener still starts and waits for a peer to dial it, so a
/// database-less node never fails just because its peer has not been
/// configured yet.
pub(crate) async fn run_edge_node(config: NodeConfig, node_id: NodeId) -> anyhow::Result<()> {
    let targets = config
        .peer_urls
        .iter()
        .map(|peer_url| PeerTarget::parse(peer_url))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let manager = config.secret_manager()?;
    let identity = NodeIdentity {
        node_id,
        role: NodeRole::Edge,
        manager,
    };

    let relay_config = config.relay_config();
    let heartbeat_timeout =
        std::time::Duration::from_secs(relay_config.worker_heartbeat_timeout_seconds.max(1));
    let (app, relay_state) = node_edge_apps(relay_config).await?;
    let context = PeerLinkContext::new(
        identity.clone(),
        PeerRoleContext::Edge(Arc::new(relay_state)),
        heartbeat_timeout,
    );
    // The actor of the forwarding hub is started here, so it exists before
    // the first peer connects and it is stopped with the node.
    if let Some(hub) = context.hub() {
        if let Err(err) = hub.start().await {
            warn!(error = %err, "the forwarding hub of the edge node did not start");
        }
    }

    // The peer endpoint is part of the single listener of the edge: the
    // database node may dial the edge as well.
    let upgrade_context = context.clone();
    let challenge_context = context.clone();
    let app = app
        .route(
            crate::config::NODE_ENDPOINT_PATH,
            get(move |request: axum::extract::Request| {
                let context = upgrade_context.clone();
                async move { peer_upgrade(context, request).await }
            }),
        )
        .route(
            super::CHALLENGE_PATH,
            get(move || {
                let context = challenge_context.clone();
                async move { peer_challenge(context).await }
            }),
        );

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|err| anyhow!("failed to bind the edge node listener: {err}"))?;
    let bound_addr = listener
        .local_addr()
        .map_err(|err| anyhow!("failed to read the address of the edge node listener: {err}"))?;
    info!(%bound_addr, "edge node listening");

    let tls_acceptor = match config.tls_mode {
        crate::config::NodeTlsMode::Off => None,
        _ => match tls::node_server_config(&config) {
            Ok(server_config) => Some(TlsAcceptor::from(server_config)),
            Err(err) => return Err(err),
        },
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (peer_shutdown_tx, peer_shutdown_rx) = watch::channel(false);
    let mut dial_tasks = Vec::new();
    for target in targets {
        let peer_tls = match target.secure {
            true => Some(tls::node_client_config(&config)?),
            false => None,
        };
        let dial_context = context.clone();
        let dial_shutdown = peer_shutdown_rx.clone();
        dial_tasks.push(tokio::spawn(async move {
            Box::pin(run_peer_dialer(
                dial_context,
                target,
                peer_tls,
                dial_shutdown,
            ))
            .await;
        }));
    }

    let signal_tx = shutdown_tx.clone();
    let signal_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = signal_tx.send(true);
    });

    let serve_shutdown = shutdown_rx.clone();
    let serve = async {
        if let Some(acceptor) = tls_acceptor {
            axum::serve(
                TlsListener::new(listener, acceptor),
                app.into_make_service_with_connect_info::<crate::relay::RemoteAddr>(),
            )
            .with_graceful_shutdown(wait_for_edge_shutdown(serve_shutdown))
            .await
            .map_err(anyhow::Error::from)
        } else {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<crate::relay::RemoteAddr>(),
            )
            .with_graceful_shutdown(wait_for_edge_shutdown(serve_shutdown))
            .await
            .map_err(anyhow::Error::from)
        }
    };
    tokio::pin!(serve);
    let outcome = tokio::select! {
        result = &mut serve => result,
        _ = wait_for_edge_shutdown(shutdown_rx.clone()) => {
            match tokio::time::timeout(std::time::Duration::from_secs(10), &mut serve).await {
                Ok(result) => result,
                Err(_) => Err(anyhow!("edge node listener did not drain before shutdown deadline")),
            }
        }
    };

    shutdown_tx.send_replace(true);
    peer_shutdown_tx.send_replace(true);
    signal_task.abort();
    let _ = signal_task.await;
    // The accepted peer sessions are closed before the dialers, so no link
    // is still running when the node stops.
    context.stop_sessions().await;
    for dial_task in dial_tasks {
        dial_task.abort();
        let _ = dial_task.await;
    }
    outcome
}

async fn wait_for_edge_shutdown(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow_and_update() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

/// Wait for the shutdown signal of the process.
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
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

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn listener_shutdown_waits_for_signal_and_observes_early_signal() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(super::wait_for_edge_shutdown(rx.clone()));
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        tx.send_replace(true);
        task.await.expect("listener shutdown completes");
        super::wait_for_edge_shutdown(rx).await;
    }
}
