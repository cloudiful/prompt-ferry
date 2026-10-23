use anyhow::{Context, anyhow};
use futures::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config, connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tracing::{error, info, warn};

use super::{
    RelayConnectionConfig,
    backoff::{
        relay_reconnect_base_delay, relay_reconnect_delay_with_jitter, wait_for_relay_ready,
    },
    handshake::negotiate_bridge_encryption,
    is_expected_relay_disconnect,
    support::{format_error_chain, ws_connect_error_detail},
};
use crate::{
    bridge_crypto::BridgeCipher,
    bridge_wire,
    config::WorkerConfig,
    protocol::BridgeMessage,
    tls,
    worker::runtime::context::{BridgeSender, ResponseLimits},
    worker::runtime::standalone::StandaloneRuntimeState,
    worker::runtime::{
        RELAY_RECONNECT_DELAY_SECONDS, WorkerRuntimeState, handle_relay_bridge_message,
    },
    worker_admin,
};

use super::support::BRIDGE_FLUSH_ALLOWANCE;

pub(super) async fn run_relay_loop(
    relay: RelayConnectionConfig,
    config: WorkerConfig,
    client: Client,
    admin_state: Option<worker_admin::AdminState>,
    standalone_state: Option<StandaloneRuntimeState>,
    runtime_state: WorkerRuntimeState,
) {
    let mut consecutive_failures = 0_u32;
    // The first attempt is usually made immediately after the relay
    // container has come up; poll for a TCP accept before paying the
    // exponential backoff to keep the worker in lock-step.
    wait_for_relay_ready(&relay.relay_url).await;
    loop {
        if runtime_state.is_shutting_down() {
            break;
        }
        let reconnect_delay = match connect_once(
            &relay,
            config.clone(),
            client.clone(),
            admin_state.clone(),
            standalone_state.clone(),
            runtime_state.clone(),
        )
        .await
        {
            Ok(()) => {
                consecutive_failures = 0;
                if let Some(state) = admin_state.as_ref() {
                    mark_relay_disconnected(state, &relay, None).await;
                }
                warn!(
                    relay_url = %relay.relay_url,
                    delay_seconds = RELAY_RECONNECT_DELAY_SECONDS,
                    "relay websocket closed; reconnecting"
                );
                relay_reconnect_base_delay(1)
            }
            Err(err) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let reconnect_delay = relay_reconnect_delay_with_jitter(consecutive_failures);
                let error_detail = format_error_chain(&err);
                if let Some(state) = admin_state.as_ref() {
                    mark_relay_disconnected(state, &relay, Some(error_detail.clone())).await;
                }
                if is_expected_relay_disconnect(&err) {
                    warn!(
                        relay_url = %relay.relay_url,
                        consecutive_failures,
                        delay_ms = reconnect_delay.as_millis() as u64,
                        error = %error_detail,
                        "relay connection ended; reconnecting"
                    );
                } else {
                    error!(
                        relay_url = %relay.relay_url,
                        consecutive_failures,
                        delay_ms = reconnect_delay.as_millis() as u64,
                        error = %error_detail,
                        "relay connection failed; reconnecting"
                    );
                }
                reconnect_delay
            }
        };

        tokio::select! {
            _ = runtime_state.wait_for_shutdown() => break,
            _ = tokio::time::sleep(reconnect_delay) => {}
        }
    }
}

pub(super) async fn connect_once(
    relay: &RelayConnectionConfig,
    config: WorkerConfig,
    client: Client,
    admin_state: Option<worker_admin::AdminState>,
    standalone_state: Option<StandaloneRuntimeState>,
    runtime_state: WorkerRuntimeState,
) -> anyhow::Result<()> {
    let mut request = relay
        .relay_url
        .as_str()
        .into_client_request()
        .context("failed to build websocket request")?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", relay.worker_token))
            .context("invalid worker token header")?,
    );

    let connector = if relay.tls_mode.enabled() {
        Some(Connector::Rustls(tls::client_config_from_pem(
            &relay.relay_url,
            relay.tls_mode,
            relay.relay_ca_pem.as_deref(),
            relay.client_cert_pem.as_deref(),
            relay.client_key_pem.as_deref(),
        )?))
    } else {
        None
    };
    let ws_config = Some(
        WebSocketConfig::default()
            .max_message_size(Some(bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES))
            .max_frame_size(Some(bridge_wire::BRIDGE_WS_MAX_FRAME_BYTES)),
    );
    let connect = if connector.is_some() {
        connect_async_tls_with_config(request, ws_config, false, connector).await
    } else {
        connect_async_with_config(request, ws_config, false).await
    };

    let (socket, _) = connect.map_err(|err| {
        let error_detail = ws_connect_error_detail(&err);
        error!(
            relay_url = %relay.relay_url,
            error = %error_detail,
            "failed to connect relay websocket"
        );
        anyhow!(err).context("failed to connect relay websocket")
    })?;
    info!(relay_url = %relay.relay_url, "connected to relay");

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (write_cipher, mut read_cipher) =
        negotiate_bridge_encryption(&mut ws_tx, &mut ws_rx, relay).await?;
    let (out_tx, control_rx, data_rx) = BridgeSender::channel();
    let flush_rx = out_tx
        .take_flush_receiver()
        .ok_or_else(|| anyhow!("bridge flush receiver is unavailable"))?;
    let (admin_tx, mut admin_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    let admin_out_tx = out_tx.clone();
    let admin_forward_task = tokio::spawn(async move {
        while let Some(message) = admin_rx.recv().await {
            if admin_out_tx.send(message).await.is_err() {
                break;
            }
        }
    });
    if let Some(state) = admin_state.as_ref() {
        worker_admin::set_bridge_sender(state, &relay.relay_key, Some(admin_tx.clone())).await;
        mark_relay_connected(state, relay).await;
        worker_admin::publish_snapshot(state).await?;
    }
    if let Some(state) = standalone_state.as_ref() {
        state
            .set_bridge_sender(&relay.relay_key, Some(admin_tx.clone()))
            .await;
        if let Some(relay_id) = relay.relay_id {
            state.mark_connected(relay_id).await;
        }
        super::super::standalone::publish_snapshot(state).await?;
    }

    let mut write_task = Some(tokio::spawn(run_bridge_writer(
        ws_tx,
        write_cipher,
        control_rx,
        data_rx,
        flush_rx,
    )));

    let mut session_error = None;
    loop {
        let next_message = tokio::select! {
            _ = runtime_state.control.wait_for_session_stop() => None,
            result = write_task.as_mut().expect("writer handle present") => {
                // The select just polled this handle to completion; take it so
                // cleanup never polls the same JoinHandle again.
                take_completed_writer(&mut write_task);
                if !runtime_state.control.is_session_stopped() {
                    session_error = Some(match result {
                        Ok(Ok(())) => anyhow!("relay bridge writer stopped"),
                        Ok(Err(err)) => err,
                        Err(err) => anyhow!(err).context("relay bridge writer task failed"),
                    });
                }
                break;
            }
            value = ws_rx.next() => value,
        };

        let Some(message) = next_message else {
            break;
        };

        match message {
            Ok(Message::Text(_)) => {
                session_error = Some(anyhow!("unexpected text relay bridge message"));
                break;
            }
            Ok(Message::Binary(bytes)) => {
                let decoded = if let Some(cipher) = read_cipher.as_mut() {
                    cipher.decrypt_message(&bytes)
                } else {
                    bridge_wire::decode_message(&bytes)
                };
                match decoded {
                    Ok(message) => {
                        let services = super::super::context::RuntimeServices::new(
                            admin_state.clone(),
                            out_tx.clone(),
                            client.clone(),
                            runtime_state.clone(),
                            ResponseLimits::from(&config),
                        );
                        let services = if let Some(state) = standalone_state.clone() {
                            services.with_standalone_state(state)
                        } else {
                            services
                        };
                        handle_relay_bridge_message(message, &config, &services).await;
                    }
                    Err(err) => {
                        session_error = Some(err.context("failed to decode relay message"));
                        break;
                    }
                }
            }
            Ok(Message::Ping(_)) => {
                let _ = out_tx.send(BridgeMessage::Pong).await;
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Err(err) => {
                session_error = Some(anyhow!(err).context("websocket read failed"));
                break;
            }
            _ => {}
        }
    }

    // Drop every remaining producer before asking the writer to flush, so the
    // acknowledgement covers only frames that were already queued.
    admin_forward_task.abort();
    let _ = admin_forward_task.await;
    if let Some(state) = admin_state.as_ref() {
        worker_admin::set_bridge_sender(state, &relay.relay_key, None).await;
    }
    if let Some(state) = standalone_state.as_ref() {
        state.set_bridge_sender(&relay.relay_key, None).await;
    }
    if runtime_state.control.is_shutting_down() {
        if !out_tx.flush(BRIDGE_FLUSH_ALLOWANCE).await {
            warn!(
                relay_url = %relay.relay_url,
                allowance_ms = BRIDGE_FLUSH_ALLOWANCE.as_millis() as u64,
                "bridge flush did not complete before the session closed"
            );
        }
    }
    drop(out_tx);
    reap_write_task(&mut write_task).await;
    if let Some(state) = standalone_state.as_ref()
        && let Some(relay_id) = relay.relay_id
    {
        state
            .mark_disconnected(relay_id, session_error.as_ref().map(ToString::to_string))
            .await;
    }
    match session_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn take_completed_writer(write_task: &mut Option<JoinHandle<anyhow::Result<()>>>) {
    write_task.take();
}

async fn reap_write_task(write_task: &mut Option<JoinHandle<anyhow::Result<()>>>) {
    let Some(task) = write_task.take() else {
        return;
    };
    task.abort();
    let _ = task.await;
}

async fn run_bridge_writer<S>(
    mut ws_tx: S,
    mut write_cipher: Option<BridgeCipher>,
    mut control_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    mut data_rx: mpsc::Receiver<super::super::bridge::BridgeData>,
    mut flush_rx: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
) -> anyhow::Result<()>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            message = control_rx.recv() => {
                let Some(message) = message else {
                    break;
                };
                write_bridge_message(&mut ws_tx, &mut write_cipher, message, "control").await?;
            }
            item = data_rx.recv() => {
                let Some(item) = item else {
                    break;
                };
                write_bridge_message(&mut ws_tx, &mut write_cipher, item.message, "data").await?;
            }
            request = flush_rx.recv() => {
                let Some(ack) = request else {
                    break;
                };
                // Every producer is gone by now, so draining both queues
                // cannot miss a later frame; acknowledging before the sink
                // flush would report bytes that are still only queued.
                while let Ok(item) = data_rx.try_recv() {
                    write_bridge_message(&mut ws_tx, &mut write_cipher, item.message, "data").await?;
                }
                while let Ok(message) = control_rx.try_recv() {
                    write_bridge_message(&mut ws_tx, &mut write_cipher, message, "control").await?;
                }
                ws_tx
                    .flush()
                    .await
                    .context("failed to flush bridge websocket sink")?;
                let _ = ack.send(());
            }
            _ = heartbeat.tick() => {
                ws_tx
                    .send(Message::Ping(Vec::new().into()))
                    .await
                    .context("failed to write bridge heartbeat")?;
            }
        }
    }
    Ok(())
}

async fn write_bridge_message<S>(
    ws_tx: &mut S,
    write_cipher: &mut Option<BridgeCipher>,
    message: BridgeMessage,
    kind: &'static str,
) -> anyhow::Result<()>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let payload = if let Some(cipher) = write_cipher.as_mut() {
        cipher.encrypt_message(&message)
    } else {
        bridge_wire::encode_message(&message)
    };
    let payload = payload.with_context(|| format!("failed to encode bridge {kind} message"))?;
    ws_tx
        .send(Message::Binary(payload.into()))
        .await
        .map_err(anyhow::Error::new)
        .with_context(|| format!("failed to write bridge {kind} message"))
}

async fn mark_relay_connected(state: &worker_admin::AdminState, relay: &RelayConnectionConfig) {
    if let Some(relay_id) = relay.relay_id {
        let mut statuses = state.managed_relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = true;
        status.last_error = None;
        status.last_connected_at = Some(chrono::Utc::now());
    }
}

async fn mark_relay_disconnected(
    state: &worker_admin::AdminState,
    relay: &RelayConnectionConfig,
    error_message: Option<String>,
) {
    if let Some(relay_id) = relay.relay_id {
        let mut statuses = state.managed_relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = false;
        status.last_disconnected_at = Some(chrono::Utc::now());
        if let Some(error_message) = error_message {
            status.last_error = Some(error_message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ResponseEnd, ResponseError};
    use futures::StreamExt;
    use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message as WsMessage};

    fn response_end() -> BridgeMessage {
        BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: "request-1".to_string(),
        })
    }

    fn terminal_error() -> BridgeMessage {
        BridgeMessage::ResponseError(ResponseError {
            request_id: "request-1".to_string(),
            status: 502,
            code: "relay_bridge_error".to_string(),
            message: "bridge failed".to_string(),
        })
    }

    #[tokio::test]
    async fn a_completed_writer_handle_is_taken_before_cleanup() {
        let mut write_task = Some(tokio::spawn(async { Ok::<(), anyhow::Error>(()) }));
        write_task
            .as_mut()
            .expect("writer handle present")
            .await
            .expect("writer task ran")
            .expect("writer succeeded");
        take_completed_writer(&mut write_task);

        reap_write_task(&mut write_task).await;
        assert!(write_task.is_none());
    }

    #[tokio::test]
    async fn a_running_writer_handle_is_aborted_and_joined_once() {
        let mut write_task = Some(tokio::spawn(async {
            std::future::pending::<()>().await;
            Ok::<(), anyhow::Error>(())
        }));

        reap_write_task(&mut write_task).await;
        assert!(write_task.is_none());
    }

    #[tokio::test]
    async fn actual_writer_drains_queued_terminal_and_control_frames_before_ack() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake relay");
        let addr = listener.local_addr().expect("fake relay addr");
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept worker");
            let mut socket = accept_async(stream).await.expect("accept websocket");
            let mut received = Vec::new();
            while received.len() < 2 {
                let Some(message) = socket.next().await else {
                    break;
                };
                if let WsMessage::Binary(bytes) = message.expect("relay reads frame") {
                    received.push(bridge_wire::decode_message(&bytes).expect("decode frame"));
                }
            }
            received
        });

        let (stream, _) = connect_async(format!("ws://{addr}"))
            .await
            .expect("connect fake relay");
        let (ws_tx, _ws_rx) = stream.split();
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("writer flush receiver");
        let writer = tokio::spawn(run_bridge_writer(
            ws_tx, None, control_rx, data_rx, flush_rx,
        ));

        sender
            .send(response_end())
            .await
            .expect("queue ResponseEnd");
        sender
            .send(terminal_error())
            .await
            .expect("queue ResponseError");
        assert!(
            sender.flush(Duration::from_secs(2)).await,
            "a healthy sink acknowledges the flush"
        );

        let received = peer.await.expect("fake relay task ran");
        assert_eq!(received.len(), 2, "both queued frames reached the sink");
        assert!(
            received
                .iter()
                .any(|message| matches!(message, BridgeMessage::ResponseEnd(_))),
            "the terminal ResponseEnd must not be dropped: {received:?}"
        );
        assert!(
            received
                .iter()
                .any(|message| matches!(message, BridgeMessage::ResponseError(_))),
            "the control ResponseError must not be dropped: {received:?}"
        );

        drop(sender);
        writer
            .await
            .expect("writer task ran")
            .expect("writer exits");
    }

    #[tokio::test]
    async fn actual_writer_reports_failed_flush_when_the_sink_fails() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("writer flush receiver");
        let writer = tokio::spawn(run_bridge_writer(
            FailingSink,
            None,
            control_rx,
            data_rx,
            flush_rx,
        ));

        let _ = sender.send(response_end()).await;
        assert!(
            !sender.flush(Duration::from_secs(2)).await,
            "a failing sink must never report a successful flush"
        );

        drop(sender);
        let _ = writer.await;
    }

    #[tokio::test]
    async fn relay_loop_reconnects_after_a_session_ends() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        use crate::config::{BridgeEncryptionMode, TlsMode, WorkerConfig};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake relay");
        let addr = listener.local_addr().expect("fake relay addr");
        let accepted = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn({
            let accepted = accepted.clone();
            async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    // The readiness probe opens a TCP connection without a
                    // websocket handshake; only completed sessions count.
                    if let Ok(socket) = accept_async(stream).await {
                        drop(socket);
                        accepted.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        let runtime_state = WorkerRuntimeState::default();
        let relay = RelayConnectionConfig {
            relay_key: "fake-relay".to_string(),
            relay_id: None,
            relay_url: format!("ws://{addr}/ws/worker"),
            worker_token: "worker-token".to_string(),
            connect_timeout_seconds: 5,
            tls_mode: TlsMode::Off,
            relay_ca_pem: None,
            client_cert_pem: None,
            client_key_pem: None,
            bridge_encryption_mode: BridgeEncryptionMode::Off,
            bridge_encryption_key: String::new(),
        };
        let loop_task = tokio::spawn(run_relay_loop(
            relay,
            WorkerConfig::default(),
            Client::new(),
            None,
            None,
            runtime_state.clone(),
        ));

        tokio::time::timeout(Duration::from_secs(10), async {
            while accepted.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("a dropped session must be followed by a reconnect");

        runtime_state.control.begin_shutdown();
        server.abort();
        tokio::time::timeout(Duration::from_secs(5), loop_task)
            .await
            .expect("relay loop exits after shutdown")
            .expect("relay loop task did not panic");
    }

    struct FailingSink;

    impl futures::Sink<WsMessage> for FailingSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn start_send(self: std::pin::Pin<&mut Self>, _item: WsMessage) -> Result<(), Self::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test sink failure",
            ))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }
}
