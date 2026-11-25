//! The two sides of an authenticated peer link.
//!
//! The database side of a link runs a dedicated executor over the shared
//! admin state and answers the peer; the database-less side of a link
//! forwards the traffic of its own clients and of its peers over the single
//! route it selected.
//!
//! Both sides speak the peer envelope of the peer wire codec, so the request
//! path of a star is the request path of the existing deployment and the
//! route controls travel next to the business traffic.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use tokio::sync::watch;
use tracing::{debug, warn};

use crate::bridge::link::{LinkEvent, LinkReceiver};
use crate::bridge_wire;
use crate::config::WorkerConfig;
use crate::protocol::{BridgeMessage, ConfigSnapshot, ResponseError};
use crate::worker::runtime::inbound::cancel_link_requests;
use crate::worker::runtime::{BridgeSender, ExecutorPump, WorkerRuntimeState};
use crate::worker_admin::{self, AdminState};

use super::admission::AdmissionToken;
use super::forwarding::{
    EnvelopeReceiver, EnvelopeSender, PEER_SEND_TIMEOUT, QueuedEnvelope, RouteHub,
    envelope_channel, write_envelopes_to_peer,
};
use super::ownership::{OwnedPeerWriter, Ownership, owned_sender};
use super::route::RouteState;
use super::transport::{PeerFrame, PeerReader, PeerWriter};
use super::wire::{PeerEnvelope, PeerEnvelopeKind, PeerPath};
use crate::worker::runtime::node::identity::NodeId;

/// How often a peer link proves that it is alive.
///
/// The interval is shorter than the timeout of the link, so a link that
/// stays healthy never reaches the timeout.
pub(crate) const PEER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

fn heartbeat_interval(timeout: Duration) -> Duration {
    (timeout / 3)
        .max(Duration::from_millis(1))
        .min(PEER_HEARTBEAT_INTERVAL)
}

/// The context every peer session of a database node shares.
pub(crate) struct DbPeerContext {
    pub(crate) config: WorkerConfig,
    pub(crate) admin_state: AdminState,
    pub(crate) client: reqwest::Client,
    pub(crate) runtime_state: WorkerRuntimeState,
    /// How long the link waits for the peer before it closes it.
    pub(crate) heartbeat_timeout: Duration,
    /// The identity of this database node, the group and the processor of
    /// every request that travels this link.
    pub(crate) node_id: NodeId,
}

/// The database side of a star link.
///
/// The link owns a dedicated executor: the requests of this edge are
/// executed over the shared admin state, and the responses travel back
/// through the budgeted sender of this link alone.
pub(crate) async fn run_db_peer_session<R: PeerReader + Send, W: PeerWriter + Send + 'static>(
    mut reader: R,
    writer: W,
    context: Arc<DbPeerContext>,
    peer_node_id: NodeId,
    _generation: u64,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let ownership = Arc::new(tokio::sync::Mutex::new(Ownership::default()));
    let (out_tx, control_rx, data_rx) = BridgeSender::channel();
    let out_tx = owned_sender(out_tx, ownership.clone());
    let flush_rx = out_tx
        .take_flush_receiver()
        .ok_or_else(|| anyhow!("the bridge flush receiver is unavailable"))?;
    let link_receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);
    let writer = OwnedPeerWriter {
        writer,
        ownership: ownership.clone(),
    };
    // The route controls of the link travel next to the business traffic,
    // but they never wait behind it.
    let (route_tx, route_rx) = envelope_channel();
    let mut writer_task =
        tokio::spawn(async move { write_link_to_peer(writer, link_receiver, route_rx).await });

    let pump = ExecutorPump::new(
        context.config.clone(),
        Some(context.admin_state.clone()),
        out_tx.clone(),
        context.client.clone(),
        context.runtime_state.clone(),
        None,
    )
    .with_node_attribution(crate::db::NodeAttribution::peer_link(
        context.node_id.as_uuid(),
        peer_node_id.as_uuid(),
    ));

    // The configuration snapshot of this link is published immediately, so
    // the edge can authenticate managed client keys without waiting for the
    // next refresh.
    //
    // The snapshot source is the single latest-value watch of the admin
    // state: a slow link never accumulates a per-link queue of publications,
    // it always forwards the newest snapshot that exists when it is ready.
    // The path of the route of the database node is the database node
    // itself: it is the end of every path.
    let terminal_path = PeerPath::new(vec![context.node_id])
        .map_err(|err| anyhow!("the path of the database node is invalid: {err}"))?;
    let snapshots = context.admin_state.subscribe_peer_snapshot();
    let mut snapshot_task = tokio::spawn(forward_peer_snapshots(
        snapshots,
        route_tx.clone(),
        terminal_path,
    ));
    let mut writer_finished = false;
    let result = tokio::select! {
        biased;
        _ = &mut stop => Ok(()),
        result = &mut writer_task => {
            writer_finished = true;
            result.unwrap_or_else(|err| Err(anyhow!("peer writer task failed: {err}")))
        },
        result = &mut snapshot_task => match result {
            Ok(Ok(())) => Err(anyhow!("the peer snapshot forwarder stopped")),
            Ok(Err(err)) => Err(err),
            Err(err) => Err(anyhow!("peer snapshot forwarder task failed: {err}")),
        },
        result = async {
    if let Err(err) = worker_admin::publish_snapshot(&context.admin_state).await {
        warn!(error = %err, "failed to publish the configuration snapshot to a peer");
    }
    run_db_frame_loop(
        &mut reader,
        &pump,
        &out_tx,
        &ownership,
        context.heartbeat_timeout,
        context.node_id,
        peer_node_id,
        &route_tx,
    ).await
        } => result,
    };
    // The forwarder is supervised: it is stopped and joined on every exit so
    // that no snapshot task outlives its link.
    snapshot_task.abort();
    let _ = snapshot_task.await;
    if !writer_finished {
        writer_task.abort();
        let _ = writer_task.await;
    }
    let (request_ids, mcp_request_ids, realtime_ids) = ownership.lock().await.active();
    cancel_link_requests(
        &context.runtime_state,
        &request_ids,
        &mcp_request_ids,
        &realtime_ids,
    )
    .await;
    result
}

/// Forward the newest published configuration snapshot to one link.
///
/// The task is the only publisher of the route updates of this link. It
/// forwards the snapshot that is already retained and then the latest one of
/// every later publication, so the edge can authenticate managed client keys
/// immediately. A send that times out or a route that closed is returned as
/// an error: the session treats it as a failure instead of silently serving a
/// stale snapshot.
async fn forward_peer_snapshots(
    mut snapshots: watch::Receiver<Option<Arc<ConfigSnapshot>>>,
    route_tx: EnvelopeSender,
    terminal_path: PeerPath,
) -> anyhow::Result<()> {
    // The advertisement counter belongs to the link, so it starts over when
    // the link is replaced.
    let mut generation = 0_u64;
    // A snapshot may have been published before this link subscribed. It is
    // still the current snapshot until a newer one replaces it.
    let retained = snapshots.borrow_and_update().clone();
    if let Some(snapshot) = retained {
        send_route_update(&route_tx, &terminal_path, &mut generation, &snapshot).await?;
    }
    loop {
        snapshots
            .changed()
            .await
            .map_err(|_| anyhow!("the peer snapshot source was closed"))?;
        // Only the newest snapshot matters. `borrow_and_update` coalesces the
        // publications that arrived while the previous send was in flight.
        let Some(snapshot) = snapshots.borrow_and_update().clone() else {
            continue;
        };
        send_route_update(&route_tx, &terminal_path, &mut generation, &snapshot).await?;
    }
}

/// Queue one snapshot as a route update of the link.
async fn send_route_update(
    route_tx: &EnvelopeSender,
    terminal_path: &PeerPath,
    generation: &mut u64,
    snapshot: &ConfigSnapshot,
) -> anyhow::Result<()> {
    *generation += 1;
    let envelope = PeerEnvelope::route_update(terminal_path.clone(), *generation, snapshot);
    match tokio::time::timeout(PEER_SEND_TIMEOUT, route_tx.send(envelope)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(anyhow!(
            "the peer route closed before the snapshot was sent"
        )),
        Err(_) => Err(anyhow!(
            "the peer snapshot could not be forwarded within the send timeout"
        )),
    }
}

/// Read the frames of a database link.
async fn run_db_frame_loop<R: PeerReader>(
    reader: &mut R,
    pump: &ExecutorPump,
    out_tx: &BridgeSender,
    ownership: &tokio::sync::Mutex<Ownership>,
    timeout: Duration,
    local: NodeId,
    peer_node_id: NodeId,
    route_tx: &EnvelopeSender,
) -> anyhow::Result<()> {
    let mut heartbeat = tokio::time::interval(heartbeat_interval(timeout));
    let mut deadline = tokio::time::Instant::now() + timeout;
    // The database node is the end of every path, so it only checks the
    // position of itself in the path of a request.
    let route = RouteState::new(local);
    loop {
        let frame = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                warn!("the peer heartbeat timed out; the link is closed");
                break;
            }
            received = reader.recv() => match received? {
                Some(frame) => frame,
                None => break,
            },
            _ = heartbeat.tick() => {
                if !matches!(tokio::time::timeout_at(deadline, out_tx.send(BridgeMessage::Ping)).await, Ok(Ok(()))) {
                    break;
                }
                continue;
            }
        };
        deadline = tokio::time::Instant::now() + timeout;
        let PeerFrame::Binary(bytes) = frame else {
            break;
        };
        let envelope = PeerEnvelope::decode(&bytes)?;
        let mut message = match envelope.kind {
            PeerEnvelopeKind::RouteUpdate | PeerEnvelopeKind::RouteWithdraw => {
                // The database node is the end of every path, so a peer never
                // advertises a route to it.
                debug!("the peer sent a route control frame to the database node");
                continue;
            }
            PeerEnvelopeKind::RequestStart => {
                let path = envelope
                    .path
                    .ok_or_else(|| anyhow!("a request start must carry a path"))?;
                let message = envelope
                    .message
                    .ok_or_else(|| anyhow!("a request start must carry a message"))?;
                if let Err(err) = route.accept_terminal_path(&path, peer_node_id) {
                    warn!(error = %err, "the path of a request start was refused");
                    refuse_start(route_tx, &message).await;
                    continue;
                }
                message
            }
            PeerEnvelopeKind::Bridge => envelope
                .message
                .ok_or_else(|| anyhow!("a bridge envelope must carry a message"))?,
        };
        let admitted = match tokio::time::timeout_at(deadline, ownership.lock()).await {
            Ok(mut ownership) => ownership.inbound(&mut message)?,
            Err(_) => break,
        };
        if !admitted {
            continue;
        }
        let cancelled = match &message {
            BridgeMessage::RequestCancel(cancel) => Some(cancel.request_id.clone()),
            BridgeMessage::McpRequestCancel(cancel) => Some(cancel.request_id.clone()),
            BridgeMessage::RealtimeSessionClose(close) => Some(close.request_id.clone()),
            _ => None,
        };
        if tokio::time::timeout_at(deadline, pump.handle(message))
            .await
            .is_err()
        {
            break;
        }
        if let Some(internal) = cancelled {
            match tokio::time::timeout_at(deadline, ownership.lock()).await {
                Ok(mut ownership) => ownership.finish(&internal),
                Err(_) => break,
            }
        }
    }
    Ok(())
}

/// Tell the peer that a request start was refused.
async fn refuse_start(route_tx: &EnvelopeSender, message: &BridgeMessage) {
    let request_id = match message {
        BridgeMessage::RequestStart(start) => start.request_id.clone(),
        BridgeMessage::McpRequestStart(start) => start.request_id.clone(),
        BridgeMessage::RealtimeSessionStart(start) => start.request_id.clone(),
        _ => return,
    };
    let error = ResponseError {
        request_id,
        status: 400,
        code: "peer_path_refused".to_string(),
        message: "the path of the request was refused".to_string(),
    };
    let envelope = PeerEnvelope::bridge(BridgeMessage::ResponseError(error));
    let _ = tokio::time::timeout(PEER_SEND_TIMEOUT, route_tx.send(envelope)).await;
}

/// The database-less side of a peer link.
///
/// The link either carries the traffic of the local clients towards the
/// selected route or the traffic of a peer that uses this node as its next
/// hop. The route controls of the link are handled by the hub of the node.
///
/// The token is the authority of the link: every action of the session
/// carries it, so a callback of a link that is no longer admitted is
/// refused. The session invalidates the token before it cleans up, so a
/// frame that is still queued cannot act after the link ended.
pub(crate) async fn run_node_peer_session<R: PeerReader + Send, W: PeerWriter + Send + 'static>(
    mut reader: R,
    writer: W,
    hub: Arc<RouteHub>,
    token: AdmissionToken,
    heartbeat_timeout: Duration,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let (out_tx, out_rx) = envelope_channel();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    if !hub.register_link(&token, out_tx.clone(), shutdown_tx).await {
        return Ok(());
    }
    let mut writer_task =
        tokio::spawn(async move { write_envelopes_to_peer(writer, out_rx).await });

    let mut writer_finished = false;
    let result = tokio::select! {
        biased;
        _ = &mut stop => Ok(()),
        _ = shutdown_rx.changed() => Ok(()),
        result = &mut writer_task => {
            writer_finished = true;
            result.unwrap_or_else(|err| Err(anyhow!("peer writer task failed: {err}")))
        },
        result = async {
    let mut heartbeat = tokio::time::interval(heartbeat_interval(heartbeat_timeout));
    let mut deadline = tokio::time::Instant::now() + heartbeat_timeout;
    loop {
        let frame = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                warn!("the peer heartbeat timed out; the link is closed");
                break;
            }
            received = reader.recv() => {
                match received? {
                    Some(frame) => frame,
                    None => break,
                }
            }
            _ = heartbeat.tick() => {
                if !matches!(
                    tokio::time::timeout_at(
                        deadline,
                        out_tx.send(PeerEnvelope::bridge(BridgeMessage::Ping))
                    ).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
                continue;
            }
        };
        deadline = tokio::time::Instant::now() + heartbeat_timeout;
        let PeerFrame::Binary(bytes) = frame else {
            break;
        };
        let envelope = PeerEnvelope::decode(&bytes)?;
        let handled = async {
            match envelope.kind {
                PeerEnvelopeKind::RouteUpdate => {
                    // The encoded frame is the unit of the control budget of
                    // the actor, so the advertisement is passed on as it
                    // arrived: the actor decodes it authoritatively when it
                    // applies it.
                    if let Err(err) = hub.apply_update_bytes(&token, bytes).await {
                        debug!(error = %err, "a route advertisement of a peer was refused");
                    }
                }
                PeerEnvelopeKind::RouteWithdraw => {
                    if let Err(err) = hub.apply_withdraw(&token, envelope.generation).await {
                        debug!(error = %err, "a route withdrawal of a peer was refused");
                    }
                }
                PeerEnvelopeKind::RequestStart => {
                    let Some(path) = envelope.path.clone() else {
                        return true;
                    };
                    let Some(message) = envelope.message.clone() else {
                        return true;
                    };
                    if let Err(err) = hub
                        .forward_start(&token, path, message.clone())
                        .await
                    {
                        warn!(error = %err, "the start of a peer request was refused");
                        refuse_start(&out_tx, &message).await;
                    }
                }
                PeerEnvelopeKind::Bridge => {
                    let Some(message) = envelope.message.clone() else {
                        return true;
                    };
                    match message {
                        BridgeMessage::Ping => {
                            let _ = out_tx.send(PeerEnvelope::bridge(BridgeMessage::Pong)).await;
                        }
                        BridgeMessage::Pong => {}
                        message => {
                            if !hub.handle_peer_message(&token, message).await {
                                // The ingress of the node refused the frame:
                                // the peer floods the node, so its link ends
                                // and its generation cleanup runs.
                                let _ = hub.invalidate_link(token.peer(), token.generation()).await;
                                return false;
                            }
                        }
                    }
                }
            }
            true
        };
        let keep = tokio::time::timeout_at(deadline, handled)
            .await
            .unwrap_or(false);
        if !keep {
            break;
        }
    }

    Ok(())
        } => result,
    };
    if !writer_finished {
        writer_task.abort();
        let _ = writer_task.await;
    }
    // The session ends: its link loses the authority before the cleanup
    // runs, so a frame that is still queued cannot act on the state of the
    // node. The invalidation is exclusive, so a publication of this link
    // that is already admitted finishes before the link dies. The cleanup
    // itself belongs to the generation of this link, so it never touches the
    // link that replaced it.
    token.invalidate_exclusive().await;
    hub.release_link(token.peer(), token.generation()).await;
    result
}

/// Write the queued bridge messages of a link to the peer.
///
/// The byte permit of a data item is released only after the sink accepted
/// the frame, the control messages keep their bypass of a saturated data
/// queue, and a flush is acknowledged only after the queued frames reached
/// the sink and the sink was flushed.
async fn write_link_to_peer<W: PeerWriter + Send + 'static>(
    mut writer: OwnedPeerWriter<W>,
    mut receiver: LinkReceiver,
    mut routes: EnvelopeReceiver,
) -> anyhow::Result<()> {
    loop {
        let outbound = tokio::select! {
            biased;
            envelope = routes.recv() => Outbound::Route(envelope),
            event = receiver.next() => Outbound::Link(event),
        };
        match outbound {
            Outbound::Route(None) => break,
            Outbound::Route(Some(envelope)) => {
                let (bytes, permit) = envelope.into_parts();
                writer.send_envelope(bytes).await?;
                drop(permit);
            }
            Outbound::Link(None) => break,
            Outbound::Link(Some(event)) => match event {
                LinkEvent::Data(item) => {
                    let (message, permit) = item.into_parts();
                    let payload = bridge_wire::encode_message(&message)?;
                    writer.send_binary(payload).await?;
                    drop(permit);
                }
                LinkEvent::Control(message) => {
                    let payload = bridge_wire::encode_message(&message)?;
                    writer.send_binary(payload).await?;
                }
                LinkEvent::Flush(ack) => {
                    let mut acknowledgements = vec![ack];
                    while let Some(event) = receiver.try_next() {
                        match event {
                            LinkEvent::Data(item) => {
                                let (message, permit) = item.into_parts();
                                let payload = bridge_wire::encode_message(&message)?;
                                writer.send_binary(payload).await?;
                                drop(permit);
                            }
                            LinkEvent::Control(message) => {
                                let payload = bridge_wire::encode_message(&message)?;
                                writer.send_binary(payload).await?;
                            }
                            LinkEvent::Flush(ack) => acknowledgements.push(ack),
                        }
                    }
                    writer.flush().await?;
                    for ack in acknowledgements {
                        let _ = ack.send(());
                    }
                }
            },
        }
    }
    Ok(())
}

/// What the writer of a database link writes next.
enum Outbound {
    /// A bridge message of the link.
    Link(Option<LinkEvent>),
    /// A route control of the link.
    Route(Option<QueuedEnvelope>),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::{Semaphore, watch};

    use crate::bridge::link::LinkReceiver;
    use crate::protocol::{BridgeMessage, ConfigSnapshot, ResponseEnd};
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::inbound::cancel_link_requests;
    use crate::worker::runtime::{BridgeSender, WorkerRuntimeState};

    use super::super::handshake::Hello;
    use super::super::transport::{MemoryPeer, PeerFrame, PeerReader, PeerWriter};
    use super::{forward_peer_snapshots, run_node_peer_session, write_link_to_peer};

    /// A sink that records the frames and can hold a frame back.
    struct GatedSink {
        frames: Arc<Mutex<Vec<PeerFrame>>>,
        gate: Arc<Semaphore>,
        flushes: Arc<AtomicUsize>,
    }

    type GatedSinkParts = (
        GatedSink,
        Arc<Mutex<Vec<PeerFrame>>>,
        Arc<Semaphore>,
        Arc<AtomicUsize>,
    );

    impl GatedSink {
        fn new() -> GatedSinkParts {
            let frames = Arc::new(Mutex::new(Vec::new()));
            let gate = Arc::new(Semaphore::new(0));
            let flushes = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    frames: frames.clone(),
                    gate: gate.clone(),
                    flushes: flushes.clone(),
                },
                frames,
                gate,
                flushes,
            )
        }
    }

    impl PeerReader for GatedSink {
        async fn recv(&mut self) -> anyhow::Result<Option<PeerFrame>> {
            Ok(None)
        }
    }

    impl PeerWriter for GatedSink {
        async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
            let permit = self
                .gate
                .acquire()
                .await
                .map_err(|_| anyhow::anyhow!("the sink is closed"))?;
            self.frames
                .lock()
                .expect("frames lock")
                .push(PeerFrame::Binary(bytes));
            permit.forget();
            Ok(())
        }

        async fn flush(&mut self) -> anyhow::Result<()> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn manager(byte: u8) -> RelaySecretManager {
        RelaySecretManager::from_key_bytes([byte; 32])
    }

    fn snapshot(version: i64) -> ConfigSnapshot {
        ConfigSnapshot {
            version,
            keys: Vec::new(),
            relay_ip_policy: Default::default(),
        }
    }

    fn terminal_path() -> super::super::wire::PeerPath {
        super::super::wire::PeerPath::new(vec![
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(1)),
        ])
        .expect("a bounded terminal path")
    }

    async fn next_route_update(
        receiver: &mut super::super::forwarding::EnvelopeReceiver,
    ) -> (u64, i64) {
        let queued = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("a route update arrives")
            .expect("the route queue is open");
        let (bytes, _permit) = queued.into_parts();
        let envelope =
            super::super::wire::PeerEnvelope::decode(&bytes).expect("a valid peer envelope");
        let snapshot = envelope.snapshot().expect("a snapshot route update");
        (envelope.generation, snapshot.version)
    }

    #[tokio::test]
    async fn a_new_link_forwards_only_the_newest_retained_snapshot_then_the_latest_change() {
        let (watch_tx, watch_rx) = watch::channel(None);
        // Publications that happen before the link subscribes are retained,
        // and only the newest one survives the coalescing of the watch.
        watch_tx.send_replace(Some(Arc::new(snapshot(1))));
        watch_tx.send_replace(Some(Arc::new(snapshot(2))));
        watch_tx.send_replace(Some(Arc::new(snapshot(3))));
        let (route_tx, mut route_rx) = super::super::forwarding::envelope_channel();
        let task = tokio::spawn(forward_peer_snapshots(watch_rx, route_tx, terminal_path()));

        assert_eq!(
            next_route_update(&mut route_rx).await,
            (1, 3),
            "the link sends the newest retained snapshot first"
        );
        watch_tx.send_replace(Some(Arc::new(snapshot(4))));
        assert_eq!(
            next_route_update(&mut route_rx).await,
            (2, 4),
            "the next publication advances the counter of the link"
        );

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn a_closed_route_fails_the_snapshot_forwarder_instead_of_stopping_silently() {
        let (_watch_tx, watch_rx) = watch::channel(Some(Arc::new(snapshot(5))));
        let (route_tx, route_rx) = super::super::forwarding::envelope_channel();
        drop(route_rx);

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            forward_peer_snapshots(watch_rx, route_tx, terminal_path()),
        )
        .await
        .expect("the forwarder returns instead of waiting forever");

        assert!(
            result.is_err(),
            "a closed route is a supervised forward failure"
        );
    }

    #[tokio::test]
    async fn either_side_may_start_a_link_and_both_sides_authenticate() {
        use super::super::auth;

        let (left, right) = MemoryPeer::pair(16);
        let (_left_reader, mut left_writer) = left.into_halves();
        let (mut right_reader, _right_writer) = right.into_halves();

        let dialer_hello = Hello::new(uuid::Uuid::from_u128(1), true, "1.2.3", 1);
        let acceptor_hello = Hello::new(uuid::Uuid::from_u128(2), false, "1.2.3", 1);

        // The peer that answers the challenge proves that it shares the
        // master secret; the proof happens before the link starts, so both
        // sides know the identity of the other one before a frame travels.
        let epoch = auth::startup_epoch();
        let challenge =
            auth::issue_challenge(&manager(7), acceptor_hello.clone(), &epoch, auth::now_ms())
                .expect("challenge");
        let (proof, _server) =
            auth::build_proof(&manager(7), &dialer_hello, &challenge, auth::now_ms())
                .expect("proof");
        let dialed = auth::verify_peer(
            &manager(7),
            &acceptor_hello,
            &epoch,
            &challenge.ticket,
            &proof,
            auth::now_ms(),
            &auth::ReplayCache::new(),
        )
        .expect("the accepting side authenticates");

        // Both sides know the identity of the other one.
        assert_eq!(dialed.hello.node_id, uuid::Uuid::from_u128(1));
        assert!(
            dialed.hello.has_database,
            "the accepting edge node sees the database node"
        );
        assert!(
            !acceptor_hello.has_database,
            "the dialing database node sees the edge node"
        );

        // The authenticated link carries the ordinary binary bridge frames.
        left_writer
            .send_binary(vec![1, 2, 3])
            .await
            .expect("the frame is written");
        let frame = right_reader.recv().await.expect("a frame arrives");
        assert_eq!(frame, Some(PeerFrame::Binary(vec![1, 2, 3])));
    }

    #[tokio::test]
    async fn queued_route_envelopes_reach_the_raw_sink_without_bridge_decoding() {
        use super::super::ownership::{OwnedPeerWriter, Ownership};

        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender
            .take_flush_receiver()
            .expect("the flush receiver is available");
        let receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);
        let (sink, frames, gate, _flushes) = GatedSink::new();
        let ownership = Arc::new(tokio::sync::Mutex::new(Ownership::default()));
        // A live business request proves that a route control neither decodes
        // nor mutates the ownership map.
        let mut request: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_start", "request_id":"live", "method":"POST", "path":"/v1/chat"
        }))
        .unwrap();
        ownership.lock().await.inbound(&mut request).unwrap();
        let live = ownership.lock().await.active().0;
        assert_eq!(live.len(), 1);

        let writer = OwnedPeerWriter {
            writer: sink,
            ownership: ownership.clone(),
        };
        let (route_tx, route_rx) = super::super::forwarding::envelope_channel();
        let task =
            tokio::spawn(async move { write_link_to_peer(writer, receiver, route_rx).await });

        let envelope =
            super::super::wire::PeerEnvelope::route_update(terminal_path(), 7, &snapshot(11));
        let expected = envelope.encode().expect("the route envelope encodes");
        route_tx.send(envelope).await.expect("the route is queued");
        gate.add_permits(1);
        drop(route_tx);

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("the writer stops after the route queue closes")
            .expect("the route write does not fail")
            .expect("the route write succeeds");
        let frames = frames.lock().unwrap();
        let Some(PeerFrame::Binary(bytes)) = frames.first() else {
            panic!("the raw sink received the route frame")
        };
        assert_eq!(
            bytes, &expected,
            "the already encoded route envelope reaches the raw sink unchanged"
        );
        let decoded =
            super::super::wire::PeerEnvelope::decode(bytes).expect("the route envelope decodes");
        assert_eq!(decoded.kind, super::PeerEnvelopeKind::RouteUpdate);
        assert_eq!(
            ownership.lock().await.active().0,
            live,
            "the route transmission leaves live request ownership untouched"
        );
    }

    #[tokio::test]
    async fn the_byte_permit_is_held_until_the_sink_accepted_the_frame() {
        let (sender, control_rx, data_rx) = BridgeSender::channel_with_byte_budget(32);
        let flush_rx = sender
            .take_flush_receiver()
            .expect("the flush receiver is available");
        let receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);
        let (sink, _frames, gate, _flushes) = GatedSink::new();
        let ownership = Arc::new(tokio::sync::Mutex::new(
            super::super::ownership::Ownership::default(),
        ));
        // The frame must belong to a live request, so the ownership-aware
        // wrapper writes it to the sink instead of dropping an unknown reply.
        let mut start: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_start", "request_id":"one", "method":"POST", "path":"/v1/chat"
        }))
        .unwrap();
        ownership.lock().await.inbound(&mut start).unwrap();
        let BridgeMessage::RequestStart(start) = start else {
            panic!("request start")
        };
        let writer = tokio::spawn(async move {
            write_link_to_peer(
                super::super::ownership::OwnedPeerWriter {
                    writer: sink,
                    ownership,
                },
                receiver,
                super::super::forwarding::envelope_channel().1,
            )
            .await
        });

        let message = BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: start.request_id,
        });
        sender
            .send_data_message(message.clone(), 32)
            .await
            .expect("the first message is queued");

        // The sink holds the frame back, so the byte permit of the message is
        // still held by the writer.
        let blocked = tokio::time::timeout(
            Duration::from_millis(100),
            sender.send_data_message(message, 32),
        )
        .await;
        assert!(
            blocked.is_err(),
            "the byte permit is retained while the frame is in flight"
        );

        // The sink accepts the frame: the permit is released and the next
        // message fits into the budget again.
        gate.add_permits(1);
        tokio::time::timeout(Duration::from_secs(5), async {
            sender
                .send_data_message(
                    BridgeMessage::ResponseEnd(ResponseEnd {
                        request_id: "two".to_string(),
                    }),
                    32,
                )
                .await
                .expect("the second message is queued");
        })
        .await
        .expect("the permit is released after the sink accepted the frame");
        writer.abort();
        let _ = writer.await;
    }

    #[tokio::test]
    async fn mapped_wire_sizes_are_reserved_until_sink_accepts() {
        use super::super::ownership::{OwnedPeerWriter, Ownership, owned_sender};
        use crate::protocol::ResponseChunk;
        for compressed in [false, true] {
            let ownership = Arc::new(tokio::sync::Mutex::new(Ownership::default()));
            let wire_id = "w".repeat(256);
            let mut request: BridgeMessage = serde_json::from_value(serde_json::json!({
                "type":"request_start", "request_id":wire_id, "method":"POST", "path":"/v1/chat"
            }))
            .unwrap();
            ownership.lock().await.inbound(&mut request).unwrap();
            let BridgeMessage::RequestStart(request) = request else {
                panic!("request start")
            };
            let mut seed = 7_u32;
            let pattern: Vec<u8> = (0..2048)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            let data = if compressed {
                pattern.repeat(40)
            } else {
                pattern[..400].to_vec()
            };
            let chunk = BridgeMessage::ResponseChunk(ResponseChunk {
                request_id: request.request_id.clone(),
                data,
            });
            let original = crate::bridge_wire::encode_message(&chunk).unwrap();
            assert_eq!(original[1] != 0, compressed);
            let mut mapped = chunk.clone();
            ownership
                .lock()
                .await
                .outbound(&mut mapped)
                .expect("owned frame");
            let wire = super::super::wire::PeerEnvelope::bridge(mapped)
                .encode()
                .unwrap();
            let budget = original.len().max(wire.len());
            let (sender, _control, mut data_rx) = BridgeSender::channel_with_byte_budget(budget);
            let sender = owned_sender(sender, ownership.clone());
            sender.send(chunk).await.expect("queue chunk");
            let (message, permit) = data_rx.recv().await.unwrap().into_parts();
            let (sink, frames, gate, _) = GatedSink::new();
            let mut writer = OwnedPeerWriter {
                writer: sink,
                ownership: ownership.clone(),
            };
            let in_flight = tokio::spawn(async move {
                writer
                    .send_binary(crate::bridge_wire::encode_message(&message).unwrap())
                    .await
                    .unwrap();
                drop(permit);
                writer
            });
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(30),
                    sender.send_data_message(BridgeMessage::Ping, budget)
                )
                .await
                .is_err()
            );
            gate.add_permits(1);
            let mut writer = in_flight.await.unwrap();
            let sent = frames.lock().unwrap().first().cloned().unwrap();
            let PeerFrame::Binary(bytes) = sent else {
                panic!("the sink received a binary frame");
            };
            let envelope = super::super::wire::PeerEnvelope::decode(&bytes)
                .expect("the peer envelope decodes");
            assert_eq!(
                envelope.message,
                super::super::wire::PeerEnvelope::decode(&wire)
                    .expect("the expected envelope decodes")
                    .message,
                "the envelope carries the owned frame"
            );
            let terminal = BridgeMessage::ResponseEnd(ResponseEnd {
                request_id: request.request_id,
            });
            sender.send(terminal).await.expect("queue terminal");
            let (message, permit) = data_rx.recv().await.unwrap().into_parts();
            gate.add_permits(1);
            writer
                .send_binary(crate::bridge_wire::encode_message(&message).unwrap())
                .await
                .unwrap();
            drop(permit);
            assert!(ownership.lock().await.active().0.is_empty());
        }
    }

    #[tokio::test]
    async fn mapped_sizes_apply_to_every_fragment() {
        use super::super::ownership::{Ownership, owned_sender};
        use crate::protocol::ResponseChunk;
        let ownership = Arc::new(tokio::sync::Mutex::new(Ownership::default()));
        let mut request: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_start", "request_id":"x".repeat(256), "method":"POST", "path":"/v1/chat"
        }))
        .unwrap();
        ownership.lock().await.inbound(&mut request).unwrap();
        let BridgeMessage::RequestStart(request) = request else {
            panic!("request")
        };
        let mut seed = 13_u32;
        let data: Vec<u8> = (0..(16 * 1024 * 1024 + 131072))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let expected_len = data.len();
        let (sender, _control, mut data_rx) = BridgeSender::channel();
        let sender = owned_sender(sender, ownership.clone());
        let producer = tokio::spawn(async move {
            sender
                .send(BridgeMessage::ResponseChunk(ResponseChunk {
                    request_id: request.request_id,
                    data,
                }))
                .await
        });
        let mut total = 0;
        let mut fragments = 0;
        while let Some(item) = data_rx.recv().await {
            let (message, permit) = item.into_parts();
            let mut mapped = message.clone();
            ownership.lock().await.outbound(&mut mapped).unwrap();
            assert!(
                permit.num_permits() >= crate::bridge_wire::encode_message(&mapped).unwrap().len()
            );
            assert!(
                permit.num_permits() >= crate::bridge_wire::encode_message(&message).unwrap().len()
            );
            if let BridgeMessage::ResponseChunk(chunk) = message {
                total += chunk.data.len();
            }
            fragments += 1;
            drop(permit);
        }
        producer.await.unwrap().unwrap();
        assert!(fragments > 1);
        assert_eq!(total, expected_len);
    }

    #[tokio::test]
    async fn a_control_message_bypasses_a_saturated_data_queue() {
        let (sender, _control_rx, _data_rx) = BridgeSender::channel_with_byte_budget(1);
        // The single byte of the budget is used by a data message.
        sender
            .send_data_message(
                BridgeMessage::ResponseEnd(ResponseEnd {
                    request_id: "one".to_string(),
                }),
                1,
            )
            .await
            .expect("the data message is queued");
        // The control message does not wait for the data budget.
        tokio::time::timeout(Duration::from_secs(1), sender.send(BridgeMessage::Ping))
            .await
            .expect("the control message bypasses the saturated data queue")
            .expect("the control message is queued");
    }

    #[tokio::test]
    async fn a_flush_is_acknowledged_after_the_sink_flushed() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender
            .take_flush_receiver()
            .expect("the flush receiver is available");
        let receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);
        let (sink, _frames, _gate, flushes) = GatedSink::new();
        let ownership = Arc::new(tokio::sync::Mutex::new(
            super::super::ownership::Ownership::default(),
        ));
        let writer = tokio::spawn(async move {
            write_link_to_peer(
                super::super::ownership::OwnedPeerWriter {
                    writer: sink,
                    ownership,
                },
                receiver,
                super::super::forwarding::envelope_channel().1,
            )
            .await
        });

        assert!(
            sender.flush(Duration::from_secs(5)).await,
            "the flush is acknowledged"
        );
        assert_eq!(
            flushes.load(Ordering::SeqCst),
            1,
            "the sink was flushed before the acknowledgement"
        );
        writer.abort();
        let _ = writer.await;
    }

    #[tokio::test]
    async fn a_peer_that_stops_answering_is_closed_after_the_timeout() {
        let (left, right) = MemoryPeer::pair(16);
        let (mut peer_reader, _peer_writer) = left.into_halves();
        let (reader, writer) = right.into_halves();
        let state = crate::relay::test_state();
        let hub = super::super::forwarding::RouteHub::new(
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(1)),
            state.clone(),
        );
        let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let session = tokio::spawn(run_node_peer_session(
            reader,
            writer,
            hub,
            super::super::admission::mint_for_test(
                crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(2)),
                1,
            ),
            Duration::from_millis(150),
            stop_rx,
        ));
        let observe = async {
            let mut pings = 0;
            while let Ok(Some(frame)) = peer_reader.recv().await {
                if let PeerFrame::Binary(bytes) = frame
                    && let Ok(envelope) = super::super::wire::PeerEnvelope::decode(&bytes)
                    && matches!(envelope.message, Some(BridgeMessage::Ping))
                {
                    pings += 1;
                }
            }
            pings
        };
        let (result, pings) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(session, observe)
        })
        .await
        .expect("heartbeat ticks must not extend the receive deadline");
        result
            .expect("session joins")
            .expect("session closes cleanly");
        assert!(pings >= 2, "the session sent repeated heartbeats");
    }

    struct FailedReader;

    impl PeerReader for FailedReader {
        async fn recv(&mut self) -> anyhow::Result<Option<PeerFrame>> {
            Err(anyhow::anyhow!("read failed"))
        }
    }

    struct FailedWriter;

    impl PeerWriter for FailedWriter {
        async fn send_binary(&mut self, _: Vec<u8>) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("sink failed"))
        }
        async fn flush(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn node_session_cleans_up_on_read_decode_and_sink_failure() {
        let state = crate::relay::test_state();
        let local_id =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(1));
        let remote =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(2));
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        let (_peer, local) = MemoryPeer::pair(4);
        let (_, writer) = local.into_halves();
        assert!(
            run_node_peer_session(
                FailedReader,
                writer,
                super::super::forwarding::RouteHub::new(local_id, state.clone()),
                super::super::admission::mint_for_test(remote, 1),
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );

        let (peer, local) = MemoryPeer::pair(4);
        let (_, mut peer_writer) = peer.into_halves();
        let (reader, writer) = local.into_halves();
        peer_writer
            .send_binary(vec![0xff])
            .await
            .expect("send malformed");
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        assert!(
            run_node_peer_session(
                reader,
                writer,
                super::super::forwarding::RouteHub::new(local_id, state.clone()),
                super::super::admission::mint_for_test(remote, 2),
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );

        let (_peer, local) = MemoryPeer::pair(4);
        let (reader, _) = local.into_halves();
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        assert!(
            run_node_peer_session(
                reader,
                FailedWriter,
                super::super::forwarding::RouteHub::new(local_id, state.clone()),
                super::super::admission::mint_for_test(remote, 3),
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn db_frame_loop_observes_timeout_and_invalid_frames() {
        use super::{Ownership, run_db_frame_loop};
        let (sender, mut control, _data) = BridgeSender::channel();
        let ownership = tokio::sync::Mutex::new(Ownership::default());
        let pump = crate::worker::runtime::ExecutorPump::new(
            crate::config::WorkerConfig::default(),
            None,
            sender.clone(),
            reqwest::Client::new(),
            WorkerRuntimeState::default(),
            None,
        );
        let (peer, local) = MemoryPeer::pair(8);
        let (_peer_reader, mut peer_writer) = peer.into_halves();
        let (mut reader, _local_writer) = local.into_halves();
        let local_id =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(1));
        let peer_id =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(2));
        let (route_tx, _route_rx) = super::super::forwarding::envelope_channel();
        run_db_frame_loop(
            &mut reader,
            &pump,
            &sender,
            &ownership,
            Duration::from_millis(150),
            local_id,
            peer_id,
            &route_tx,
        )
        .await
        .unwrap();
        let mut pings = 0;
        while let Ok(message) = control.try_recv() {
            if matches!(message, BridgeMessage::Ping) {
                pings += 1;
            }
        }
        assert!(pings >= 2);
        peer_writer.send_binary(vec![0xff]).await.unwrap();
        assert!(
            run_db_frame_loop(
                &mut reader,
                &pump,
                &sender,
                &ownership,
                Duration::from_secs(1),
                local_id,
                peer_id,
                &route_tx,
            )
            .await
            .is_err()
        );
        assert!(
            run_db_frame_loop(
                &mut FailedReader,
                &pump,
                &sender,
                &ownership,
                Duration::from_secs(1),
                local_id,
                peer_id,
                &route_tx,
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn db_frame_loops_isolate_colliding_wire_ids_and_cancel_only_owner() {
        use super::Ownership;
        use super::run_db_frame_loop;
        let runtime = WorkerRuntimeState::default();
        let mut writers = Vec::new();
        let mut tasks = Vec::new();
        let mut owners = Vec::new();
        let local_id =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(1));
        let peer_id =
            crate::worker::runtime::node::identity::NodeId::from_uuid(uuid::Uuid::from_u128(2));
        for _ in 0..2 {
            let (peer, local) = MemoryPeer::pair(16);
            let (_peer_reader, peer_writer) = peer.into_halves();
            let (mut reader, _writer) = local.into_halves();
            let (sender, control, data) = BridgeSender::channel();
            let ownership = Arc::new(tokio::sync::Mutex::new(Ownership::default()));
            let own = ownership.clone();
            let state = runtime.clone();
            let pump = crate::worker::runtime::ExecutorPump::new(
                crate::config::WorkerConfig::default(),
                None,
                sender.clone(),
                reqwest::Client::new(),
                state,
                None,
            );
            let (route_tx, _route_rx) = super::super::forwarding::envelope_channel();
            tasks.push(tokio::spawn(async move {
                let _keep_receivers = (control, data, _writer);
                run_db_frame_loop(
                    &mut reader,
                    &pump,
                    &sender,
                    &own,
                    Duration::from_secs(5),
                    local_id,
                    peer_id,
                    &route_tx,
                )
                .await
            }));
            owners.push(ownership);
            writers.push(peer_writer);
        }
        let start: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_start", "request_id":"collision", "method":"POST", "path":"/v1/chat"
        }))
        .unwrap();
        // The start of a request travels in an envelope with the path of the
        // request, so the database node knows the position of itself in it.
        let path =
            super::super::wire::PeerPath::new(vec![peer_id, local_id]).expect("a bounded path");
        let start_envelope = super::super::wire::PeerEnvelope::request_start(path, start);
        for writer in &mut writers {
            writer
                .send_binary(start_envelope.encode().unwrap())
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while runtime.request_cancellations.lock().await.len() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both distinct requests registered");
        let a = owners[0]
            .lock()
            .await
            .active()
            .0
            .into_iter()
            .next()
            .unwrap();
        let b = owners[1]
            .lock()
            .await
            .active()
            .0
            .into_iter()
            .next()
            .unwrap();
        assert_ne!(a, b);
        let cancel: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_cancel", "request_id":"collision", "reason":"cancelled"
        }))
        .unwrap();
        writers[0]
            .send_binary(
                super::super::wire::PeerEnvelope::bridge(cancel)
                    .encode()
                    .unwrap(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !owners[0].lock().await.active().0.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancel completed");
        assert!(!runtime.request_cancellations.lock().await.contains_key(&a));
        assert!(runtime.request_cancellations.lock().await.contains_key(&b));
        let (http, mcp, realtime) = owners[1].lock().await.active();
        cancel_link_requests(&runtime, &http, &mcp, &realtime).await;
        drop(writers);
        for task in tasks {
            task.await.unwrap().unwrap();
        }
    }

    #[test]
    fn the_heartbeat_interval_is_shorter_than_the_timeout() {
        let timeout = Duration::from_secs(90);
        assert!(
            super::PEER_HEARTBEAT_INTERVAL < timeout,
            "a healthy link never reaches the timeout"
        );
    }

    #[tokio::test]
    async fn a_disconnect_cancels_only_the_requests_of_its_own_link() {
        let runtime_state = WorkerRuntimeState::default();
        let own = crate::worker::runtime::request_assembly::RequestCancellation::default();
        let other = crate::worker::runtime::request_assembly::RequestCancellation::default();
        runtime_state
            .request_cancellations
            .lock()
            .await
            .insert("own".to_string(), own.clone());
        runtime_state
            .request_cancellations
            .lock()
            .await
            .insert("other".to_string(), other.clone());

        let mut own_ids = std::collections::HashSet::new();
        own_ids.insert("own".to_string());
        cancel_link_requests(
            &runtime_state,
            &own_ids,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        )
        .await;

        assert!(own.is_cancelled(), "the request of the link is cancelled");
        assert!(
            !other.is_cancelled(),
            "the request of another link keeps running"
        );
        assert!(
            !runtime_state
                .request_cancellations
                .lock()
                .await
                .contains_key("own"),
            "the cancelled request is removed"
        );
        assert!(
            runtime_state
                .request_cancellations
                .lock()
                .await
                .contains_key("other"),
            "the request of another link is untouched"
        );
    }
}
