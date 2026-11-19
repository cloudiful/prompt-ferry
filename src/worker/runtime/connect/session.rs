#[cfg(test)]
use anyhow::Context;
#[cfg(test)]
use futures::SinkExt;
use reqwest::Client;
#[cfg(test)]
use std::time::Duration;
use tokio::sync::mpsc;
#[cfg(test)]
use tokio::task::JoinHandle;
#[cfg(test)]
use tokio_tungstenite::tungstenite::Message;

use crate::{
    config::WorkerConfig,
    protocol::BridgeMessage,
    worker::runtime::context::{BridgeSender, ResponseLimits},
    worker::runtime::standalone::StandaloneRuntimeState,
    worker::runtime::{WorkerRuntimeState, handle_relay_bridge_message},
    worker_admin,
};

#[cfg(test)]
use crate::{
    bridge::link::{LinkEvent, LinkReceiver},
    bridge_crypto::BridgeCipher,
    bridge_wire,
};

/// Transport-independent executor entry point.
///
/// The websocket session and a future local node both drive the same
/// [`handle_relay_bridge_message`] dispatcher with the same services and the
/// same budgeted outbound sender. The pump only owns request consumption; the
/// transport keeps its reader, crypto, heartbeat and flush handling.
pub(crate) struct ExecutorPump {
    config: WorkerConfig,
    services: super::super::context::RuntimeServices,
}

impl ExecutorPump {
    pub(crate) fn new(
        config: WorkerConfig,
        admin_state: Option<worker_admin::AdminState>,
        out_tx: BridgeSender,
        client: Client,
        runtime_state: WorkerRuntimeState,
        standalone_state: Option<StandaloneRuntimeState>,
    ) -> Self {
        let services = super::super::context::RuntimeServices::new(
            admin_state,
            out_tx,
            client,
            runtime_state,
            ResponseLimits::from(&config),
        );
        let services = match standalone_state {
            Some(state) => services.with_standalone_state(state),
            None => services,
        };
        Self { config, services }
    }

    pub(crate) async fn handle(&self, message: BridgeMessage) {
        handle_relay_bridge_message(message, &self.config, &self.services).await;
    }

    /// Bind the node ownership of this link. The local executor of a database
    /// node binds the node itself; the database side of a peer link binds the
    /// authenticated remote edge as its ingress. The legacy relay connection
    /// never calls it and stores no attribution.
    pub(crate) fn with_node_attribution(mut self, attribution: crate::db::NodeAttribution) -> Self {
        self.services = self.services.with_node_attribution(attribution);
        self
    }

    /// Consume typed bridge messages until the inbound half closes.
    pub(crate) async fn run(&self, mut inbound: mpsc::Receiver<BridgeMessage>) {
        while let Some(message) = inbound.recv().await {
            self.handle(message).await;
        }
    }
}

#[cfg(test)]
fn take_completed_writer(write_task: &mut Option<JoinHandle<anyhow::Result<()>>>) {
    write_task.take();
}

#[cfg(test)]
async fn reap_write_task(write_task: &mut Option<JoinHandle<anyhow::Result<()>>>) {
    let Some(task) = write_task.take() else {
        return;
    };
    task.abort();
    let _ = task.await;
}

#[cfg(test)]
async fn run_bridge_writer<S>(
    mut ws_tx: S,
    mut write_cipher: Option<BridgeCipher>,
    mut receiver: LinkReceiver,
) -> anyhow::Result<()>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            event = receiver.next() => {
                let Some(event) = event else {
                    break;
                };
                match event {
                    LinkEvent::Data(item) => {
                        let (message, permit) = item.into_parts();
                        write_bridge_message(&mut ws_tx, &mut write_cipher, &message, "data").await?;
                        // The item's byte permit is released only here, after
                        // the sink accepted the frame.
                        drop(permit);
                    }
                    LinkEvent::Control(message) => {
                        write_bridge_message(&mut ws_tx, &mut write_cipher, &message, "control").await?;
                    }
                    LinkEvent::Flush(ack) => {
                        // Every producer is gone by now, so draining both queues
                        // cannot miss a later frame; acknowledging before the sink
                        // flush would report bytes that are still only queued.
                        let mut acknowledgements = vec![ack];
                        while let Some(event) = receiver.try_next() {
                            match event {
                                LinkEvent::Data(item) => {
                                    let (message, permit) = item.into_parts();
                                    write_bridge_message(&mut ws_tx, &mut write_cipher, &message, "data").await?;
                                    drop(permit);
                                }
                                LinkEvent::Control(message) => {
                                    write_bridge_message(&mut ws_tx, &mut write_cipher, &message, "control").await?;
                                }
                                LinkEvent::Flush(ack) => acknowledgements.push(ack),
                            }
                        }
                        ws_tx
                            .flush()
                            .await
                            .context("failed to flush bridge websocket sink")?;
                        for ack in acknowledgements {
                            let _ = ack.send(());
                        }
                    }
                }
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

#[cfg(test)]
async fn write_bridge_message<S>(
    ws_tx: &mut S,
    write_cipher: &mut Option<BridgeCipher>,
    message: &BridgeMessage,
    kind: &'static str,
) -> anyhow::Result<()>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let payload = if let Some(cipher) = write_cipher.as_mut() {
        cipher.encrypt_message(message)
    } else {
        bridge_wire::encode_message(message)
    };
    let payload = payload.with_context(|| format!("failed to encode bridge {kind} message"))?;
    ws_tx
        .send(Message::Binary(payload.into()))
        .await
        .map_err(anyhow::Error::new)
        .with_context(|| format!("failed to write bridge {kind} message"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ResponseEnd;

    use tokio_tungstenite::tungstenite::Message as WsMessage;

    fn response_end() -> BridgeMessage {
        BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: "request-1".to_string(),
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
    async fn actual_writer_reports_failed_flush_when_the_sink_fails() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("writer flush receiver");
        let writer = tokio::spawn(run_bridge_writer(
            FailingSink,
            None,
            LinkReceiver::new(control_rx, data_rx, flush_rx),
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
    async fn flush_is_acknowledged_after_queued_data_reaches_and_flushes_the_sink() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("writer flush receiver");
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = RecordingSink {
            events: events.clone(),
        };
        let writer = tokio::spawn(run_bridge_writer(
            sink,
            None,
            LinkReceiver::new(control_rx, data_rx, flush_rx),
        ));

        sender
            .send(response_end())
            .await
            .expect("queued data before the flush");
        assert!(
            sender.flush(Duration::from_secs(2)).await,
            "the writer acknowledges a completed sink flush"
        );

        {
            let events = events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let data_index = events
                .iter()
                .position(|event| *event == SinkEvent::Data)
                .expect("the queued data reached the sink");
            let flush_index = events
                .iter()
                .position(|event| *event == SinkEvent::Flush)
                .expect("the sink was flushed");
            assert!(
                data_index < flush_index,
                "the flush marker follows the preceding queued data: {events:?}"
            );
        }

        drop(sender);
        let _ = writer.await;
    }

    #[tokio::test]
    async fn executor_pump_handles_ping_with_the_existing_dispatcher() {
        let (sender, mut control_rx, _data_rx) = BridgeSender::channel();
        let pump = ExecutorPump::new(
            crate::config::WorkerConfig::default(),
            None,
            sender,
            reqwest::Client::new(),
            super::super::super::WorkerRuntimeState::default(),
            None,
        );
        let (request_tx, request_rx) = mpsc::channel(4);
        let task = tokio::spawn(async move { pump.run(request_rx).await });

        request_tx
            .send(crate::protocol::BridgeMessage::Ping)
            .await
            .expect("request reaches the pump");
        let response = tokio::time::timeout(Duration::from_secs(1), control_rx.recv())
            .await
            .expect("the pump answers")
            .expect("control channel stays open");
        assert_eq!(response, crate::protocol::BridgeMessage::Pong);

        drop(request_tx);
        task.await
            .expect("the pump stops when its inbound half closes");
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum SinkEvent {
        Data,
        Flush,
    }

    struct RecordingSink {
        events: std::sync::Arc<std::sync::Mutex<Vec<SinkEvent>>>,
    }

    impl futures::Sink<WsMessage> for RecordingSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn start_send(self: std::pin::Pin<&mut Self>, item: WsMessage) -> Result<(), Self::Error> {
            if matches!(item, WsMessage::Binary(_)) {
                self.get_mut()
                    .events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(SinkEvent::Data);
            }
            Ok(())
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.get_mut()
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(SinkEvent::Flush);
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
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
