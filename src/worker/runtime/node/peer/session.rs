//! The two sides of an authenticated star link.
//!
//! The database side of a link runs a dedicated executor over the shared
//! admin state and answers the edge node; the edge side registers the remote
//! database node as the executor of its relay state and forwards the
//! business traffic of its own clients to it.
//!
//! Both sides speak the same typed bridge messages as a relay and a remote
//! worker do, so the request path of a star is the request path of the
//! existing deployment.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::bridge::link::{BRIDGE_LINK_BUFFER, LinkEvent, LinkReceiver};
use crate::bridge_wire;
use crate::config::WorkerConfig;
use crate::protocol::BridgeMessage;
use crate::relay::{
    AppState, WorkerSender, cleanup_worker, dispatch_worker_bridge_message, register_worker,
};
use crate::worker::runtime::inbound::cancel_link_requests;
use crate::worker::runtime::{BridgeSender, ExecutorPump, WorkerRuntimeState};
use crate::worker_admin::{self, AdminState};

use super::ownership::{OwnedPeerWriter, Ownership, owned_sender};
use super::transport::{PeerFrame, PeerReader, PeerWriter};
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
    generation: u64,
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
    let mut writer_task =
        tokio::spawn(async move { write_link_to_peer(writer, link_receiver).await });

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
    let (admin_tx, mut admin_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    let admin_out_tx = out_tx.clone();
    let admin_forward_task = tokio::spawn(async move {
        while let Some(message) = admin_rx.recv().await {
            if admin_out_tx.send(message).await.is_err() {
                break;
            }
        }
    });
    // The key carries the generation, so the cleanup of a replaced link can
    // never remove the snapshot target of the link that replaced it.
    let snapshot_key = format!("peer-{peer_node_id}#{generation}");
    worker_admin::set_bridge_sender(&context.admin_state, &snapshot_key, Some(admin_tx)).await;
    let mut writer_finished = false;
    let result = tokio::select! {
        biased;
        _ = &mut stop => Ok(()),
        result = &mut writer_task => {
            writer_finished = true;
            result.unwrap_or_else(|err| Err(anyhow!("peer writer task failed: {err}")))
        },
        result = async {
    if let Err(err) = worker_admin::publish_snapshot(&context.admin_state).await {
        warn!(error = %err, "failed to publish the configuration snapshot to a peer");
    }
    run_db_frame_loop(&mut reader, &pump, &out_tx, &ownership, context.heartbeat_timeout).await
        } => result,
    };
    worker_admin::set_bridge_sender(&context.admin_state, &snapshot_key, None).await;
    admin_forward_task.abort();
    let _ = admin_forward_task.await;
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

async fn run_db_frame_loop<R: PeerReader>(
    reader: &mut R,
    pump: &ExecutorPump,
    out_tx: &BridgeSender,
    ownership: &tokio::sync::Mutex<Ownership>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let mut heartbeat = tokio::time::interval(heartbeat_interval(timeout));
    let mut deadline = tokio::time::Instant::now() + timeout;
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
        let mut message = bridge_wire::decode_message(&bytes)?;
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

/// The edge side of a star link.
///
/// The remote database node is registered as the executor of the local relay
/// state, so the business traffic of the local clients travels the ordinary
/// relay path and reaches the database node unchanged.
pub(crate) async fn run_edge_peer_session<R: PeerReader + Send, W: PeerWriter + Send + 'static>(
    mut reader: R,
    writer: W,
    relay_state: AppState,
    worker_id: usize,
    heartbeat_timeout: Duration,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let (request_tx, request_rx) = mpsc::channel::<BridgeMessage>(BRIDGE_LINK_BUFFER);
    let request_tx: WorkerSender = request_tx;
    if !register_worker(&relay_state, worker_id, request_tx).await {
        return Err(anyhow!(
            "the remote database node was refused as a relay worker"
        ));
    }
    info!(
        worker_id,
        "the remote database node is registered as the executor"
    );
    // The control channel carries the answers of the link itself, so the
    // heartbeat of the peer never travels through the relay state.
    let (control_tx, control_rx) = mpsc::channel::<BridgeMessage>(8);
    let mut writer_task =
        tokio::spawn(async move { write_requests_to_peer(writer, request_rx, control_rx).await });

    let mut writer_finished = false;
    let result = tokio::select! {
        biased;
        _ = &mut stop => Ok(()),
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
                if !matches!(tokio::time::timeout_at(deadline, control_tx.send(BridgeMessage::Ping)).await, Ok(Ok(()))) {
                    break;
                }
                continue;
            }
        };
        deadline = tokio::time::Instant::now() + heartbeat_timeout;
        let PeerFrame::Binary(bytes) = frame else {
            break;
        };
        let message = bridge_wire::decode_message(&bytes)?;
        let handled = async {
            match message {
                BridgeMessage::Ping => {
                    let _ = control_tx.send(BridgeMessage::Pong).await;
                }
                message => dispatch_worker_bridge_message(&relay_state, message).await,
            }
        };
        if tokio::time::timeout_at(deadline, handled).await.is_err() {
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
    cleanup_worker(&relay_state, worker_id).await;
    result
}

/// Write the queued bridge messages of a link to the peer.
///
/// The byte permit of a data item is released only after the sink accepted
/// the frame, the control messages keep their bypass of a saturated data
/// queue, and a flush is acknowledged only after the queued frames reached
/// the sink and the sink was flushed.
async fn write_link_to_peer<W: PeerWriter + Send + 'static>(
    mut writer: W,
    mut receiver: LinkReceiver,
) -> anyhow::Result<()> {
    loop {
        let Some(event) = receiver.next().await else {
            break;
        };
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
        }
    }
    Ok(())
}

/// Write the business requests of the local relay to the peer.
///
/// The writer also carries the heartbeat of the link and the answers of the
/// link itself, so a control message never waits behind the business
/// traffic.
async fn write_requests_to_peer<W: PeerWriter + Send + 'static>(
    mut writer: W,
    mut requests: mpsc::Receiver<BridgeMessage>,
    mut control: mpsc::Receiver<BridgeMessage>,
) -> anyhow::Result<()> {
    let mut requests_open = true;
    loop {
        tokio::select! {
            message = requests.recv(), if requests_open => {
                match message {
                    Some(message) => {
                        let payload = bridge_wire::encode_message(&message)?;
                        writer.send_binary(payload).await?;
                    }
                    None => {
                        // The relay closed its side of the link, but the
                        // peer link itself stays alive.
                        requests_open = false;
                    }
                }
            }
            message = control.recv() => {
                let Some(message) = message else {
                    break;
                };
                let payload = bridge_wire::encode_message(&message)?;
                writer.send_binary(payload).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::Semaphore;

    use crate::bridge::link::LinkReceiver;
    use crate::protocol::{BridgeMessage, ResponseEnd};
    use crate::relay_secrets::RelaySecretManager;
    use crate::worker::runtime::inbound::cancel_link_requests;
    use crate::worker::runtime::{BridgeSender, WorkerRuntimeState};

    use super::super::handshake::Hello;
    use super::super::transport::{MemoryPeer, PeerFrame, PeerReader, PeerWriter};
    use super::{run_edge_peer_session, write_link_to_peer};

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
    async fn the_byte_permit_is_held_until_the_sink_accepted_the_frame() {
        let (sender, control_rx, data_rx) = BridgeSender::channel_with_byte_budget(32);
        let flush_rx = sender
            .take_flush_receiver()
            .expect("the flush receiver is available");
        let receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);
        let (sink, _frames, gate, _flushes) = GatedSink::new();
        let writer = tokio::spawn(async move { write_link_to_peer(sink, receiver).await });

        let message = BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: "one".to_string(),
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
            let wire = crate::bridge_wire::encode_message(&mapped).unwrap();
            let budget = original.len().max(wire.len());
            let (sender, _control, mut data_rx) = BridgeSender::channel_with_byte_budget(budget);
            let sender = owned_sender(sender, ownership.clone());
            sender.send(chunk).await.expect("queue chunk");
            let (message, permit) = data_rx.recv().await.unwrap().into_parts();
            assert_eq!(permit.num_permits(), budget);
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
            assert_eq!(sent, PeerFrame::Binary(wire));
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
        let writer = tokio::spawn(async move { write_link_to_peer(sink, receiver).await });

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
        let (_stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let session = tokio::spawn(run_edge_peer_session(
            reader,
            writer,
            state.clone(),
            7,
            Duration::from_millis(150),
            stop_rx,
        ));
        let observe = async {
            let mut pings = 0;
            while let Ok(Some(frame)) = peer_reader.recv().await {
                if let PeerFrame::Binary(bytes) = frame
                    && matches!(
                        crate::bridge_wire::decode_message(&bytes),
                        Ok(BridgeMessage::Ping)
                    )
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
        assert!(state.inner.workers.lock().await.is_empty());
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
    async fn edge_session_cleans_up_on_read_decode_and_sink_failure() {
        let state = crate::relay::test_state();
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        let (_peer, local) = MemoryPeer::pair(4);
        let (_, writer) = local.into_halves();
        assert!(
            run_edge_peer_session(
                FailedReader,
                writer,
                state.clone(),
                1,
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );
        assert!(state.inner.workers.lock().await.is_empty());

        let (peer, local) = MemoryPeer::pair(4);
        let (_, mut peer_writer) = peer.into_halves();
        let (reader, writer) = local.into_halves();
        peer_writer
            .send_binary(vec![0xff])
            .await
            .expect("send malformed");
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        assert!(
            run_edge_peer_session(
                reader,
                writer,
                state.clone(),
                2,
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );
        assert!(state.inner.workers.lock().await.is_empty());

        let (_peer, local) = MemoryPeer::pair(4);
        let (reader, _) = local.into_halves();
        let (_stop, stop_rx) = tokio::sync::oneshot::channel();
        assert!(
            run_edge_peer_session(
                reader,
                FailedWriter,
                state.clone(),
                3,
                Duration::from_secs(1),
                stop_rx
            )
            .await
            .is_err()
        );
        assert!(state.inner.workers.lock().await.is_empty());
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
        run_db_frame_loop(
            &mut reader,
            &pump,
            &sender,
            &ownership,
            Duration::from_millis(150),
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
                Duration::from_secs(1)
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
                Duration::from_secs(1)
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
            tasks.push(tokio::spawn(async move {
                let _keep_receivers = (control, data, _writer);
                run_db_frame_loop(&mut reader, &pump, &sender, &own, Duration::from_secs(5)).await
            }));
            owners.push(ownership);
            writers.push(peer_writer);
        }
        let start: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"request_start", "request_id":"collision", "method":"POST", "path":"/v1/chat"
        }))
        .unwrap();
        for writer in &mut writers {
            writer
                .send_binary(crate::bridge_wire::encode_message(&start).unwrap())
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
            .send_binary(crate::bridge_wire::encode_message(&cancel).unwrap())
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
