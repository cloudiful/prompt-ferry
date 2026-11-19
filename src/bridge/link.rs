use tokio::sync::{mpsc, oneshot};

use crate::{
    protocol::BridgeMessage,
    worker::{BridgeData, BridgeSender},
};

/// Existing relay worker-send channel capacity, shared by the socket and
/// in-memory worker links so local execution keeps the same request backpressure.
pub(crate) const BRIDGE_LINK_BUFFER: usize = 64;

/// One item read from the executor-to-relay half of a link.
///
/// Control messages are polled before data, mirroring [`BridgeSender`]'s
/// control bypass: control and data are not totally ordered on the wire, and a
/// flush marker only promises that everything it drained before the sink flush
/// was written.
pub(crate) enum LinkEvent {
    Data(BridgeData),
    Control(BridgeMessage),
    Flush(oneshot::Sender<()>),
}

/// Receiver side of a bridge link.
///
/// Both the websocket writer and the local relay-side dispatcher consume the
/// same bounded data queue, unbounded control queue and ordered flush marker.
pub(crate) struct LinkReceiver {
    control_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    data_rx: mpsc::Receiver<BridgeData>,
    flush_rx: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    control_closed: bool,
    data_closed: bool,
    flush_closed: bool,
}

impl LinkReceiver {
    pub(crate) fn new(
        control_rx: mpsc::UnboundedReceiver<BridgeMessage>,
        data_rx: mpsc::Receiver<BridgeData>,
        flush_rx: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    ) -> Self {
        Self {
            control_rx,
            data_rx,
            flush_rx,
            control_closed: false,
            data_closed: false,
            flush_closed: false,
        }
    }

    /// Wait for the next queued control message, data item, or flush request.
    ///
    /// A data item owns its byte permit until the caller drops it, so a writer
    /// that holds it through the sink write keeps the outbound budget honest.
    pub(crate) async fn next(&mut self) -> Option<LinkEvent> {
        loop {
            if self.control_closed && self.data_closed && self.flush_closed {
                return None;
            }
            tokio::select! {
                biased;
                message = self.control_rx.recv(), if !self.control_closed => {
                    match message {
                        Some(message) => return Some(LinkEvent::Control(message)),
                        None => self.control_closed = true,
                    }
                }
                item = self.data_rx.recv(), if !self.data_closed => {
                    match item {
                        Some(item) => return Some(LinkEvent::Data(item)),
                        None => self.data_closed = true,
                    }
                }
                ack = self.flush_rx.recv(), if !self.flush_closed => {
                    match ack {
                        Some(ack) => return Some(LinkEvent::Flush(ack)),
                        None => self.flush_closed = true,
                    }
                }
            }
        }
    }

    /// Non-blocking read used while draining the queues before acknowledging a
    /// flush marker. Control keeps priority over data and a nested flush request
    /// is collected by the caller and acknowledged after the same sink flush.
    pub(crate) fn try_next(&mut self) -> Option<LinkEvent> {
        if !self.control_closed {
            match self.control_rx.try_recv() {
                Ok(message) => return Some(LinkEvent::Control(message)),
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => self.control_closed = true,
            }
        }
        if !self.data_closed {
            match self.data_rx.try_recv() {
                Ok(item) => return Some(LinkEvent::Data(item)),
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => self.data_closed = true,
            }
        }
        if !self.flush_closed {
            match self.flush_rx.try_recv() {
                Ok(ack) => return Some(LinkEvent::Flush(ack)),
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => self.flush_closed = true,
            }
        }
        None
    }
}

/// Both halves of a local, in-memory bridge link.
///
/// `relay_request_tx` is the relay's worker sender and is bounded by
/// [`BRIDGE_LINK_BUFFER`]; `executor_response_tx` is the existing budgeted
/// [`BridgeSender`]. The local executor pump consumes `executor_request_rx`,
/// while the relay dispatcher consumes `relay_response_rx`.
pub(crate) struct InMemoryLink {
    pub(crate) relay_request_tx: mpsc::Sender<BridgeMessage>,
    pub(crate) executor_request_rx: mpsc::Receiver<BridgeMessage>,
    pub(crate) executor_response_tx: BridgeSender,
    pub(crate) relay_response_rx: LinkReceiver,
}

pub(crate) fn in_memory_link() -> InMemoryLink {
    let (relay_request_tx, executor_request_rx) = mpsc::channel(BRIDGE_LINK_BUFFER);
    let (executor_response_tx, control_rx, data_rx) = BridgeSender::channel();
    let flush_rx = executor_response_tx
        .take_flush_receiver()
        .expect("a fresh in-memory link owns its flush receiver");
    InMemoryLink {
        relay_request_tx,
        executor_request_rx,
        executor_response_tx,
        relay_response_rx: LinkReceiver::new(control_rx, data_rx, flush_rx),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{BRIDGE_LINK_BUFFER, InMemoryLink, LinkEvent, LinkReceiver, in_memory_link};
    use crate::{
        protocol::{BridgeMessage, BridgeRequestCancel, ResponseStart},
        worker::BridgeSender,
    };

    fn response_start(request_id: &str) -> BridgeMessage {
        BridgeMessage::ResponseStart(ResponseStart {
            request_id: request_id.to_string(),
            status: 200,
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        })
    }

    fn request_cancel(request_id: &str) -> BridgeMessage {
        BridgeMessage::RequestCancel(BridgeRequestCancel {
            request_id: request_id.to_string(),
            reason: "client_cancel".to_string(),
            response_started: false,
        })
    }

    fn response_chunk(data: Vec<u8>) -> BridgeMessage {
        BridgeMessage::ResponseChunk(crate::protocol::ResponseChunk {
            request_id: "request-1".to_string(),
            data,
        })
    }

    fn terminal_error() -> BridgeMessage {
        BridgeMessage::ResponseError(crate::protocol::ResponseError {
            request_id: "request-1".to_string(),
            status: 502,
            code: "relay_bridge_error".to_string(),
            message: "bridge failed".to_string(),
        })
    }

    #[tokio::test]
    async fn in_memory_link_preserves_typed_requests_responses_and_cancellations() {
        let InMemoryLink {
            relay_request_tx,
            mut executor_request_rx,
            executor_response_tx,
            mut relay_response_rx,
        } = in_memory_link();

        let cancel = request_cancel("request-1");
        relay_request_tx
            .send(cancel.clone())
            .await
            .expect("relay queues a typed request");
        assert_eq!(
            executor_request_rx.recv().await,
            Some(cancel),
            "the executor receives the same typed request"
        );

        let start = response_start("request-1");
        executor_response_tx
            .send(start.clone())
            .await
            .expect("executor queues a typed response");
        match relay_response_rx.next().await {
            Some(LinkEvent::Data(item)) => {
                let (message, permit) = item.into_parts();
                assert_eq!(message, start);
                drop(permit);
            }
            _ => panic!("the relay receives the typed response"),
        }
    }

    #[tokio::test]
    async fn data_byte_permit_is_retained_until_the_receiver_drops_the_item() {
        let (sender, control_rx, data_rx) = BridgeSender::channel_with_byte_budget(4096);
        let flush_rx = sender.take_flush_receiver().expect("flush receiver");
        let mut receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);

        sender
            .send(response_chunk(vec![0u8; 3000]))
            .await
            .expect("first chunk fits the budget");
        let in_flight = match receiver.next().await {
            Some(LinkEvent::Data(item)) => item,
            _ => panic!("first data item arrives"),
        };

        let pending_sender = sender.clone();
        let pending =
            tokio::spawn(async move { pending_sender.send(response_chunk(vec![0u8; 3000])).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !pending.is_finished(),
            "the permit is still held by the in-flight item"
        );

        drop(in_flight);
        pending
            .await
            .expect("pending send task ran")
            .expect("the second chunk proceeds once the permit is released");
        match receiver.next().await {
            Some(LinkEvent::Data(item)) => drop(item),
            _ => panic!("second data item arrives"),
        }
    }

    #[tokio::test]
    async fn control_messages_bypass_a_saturated_data_queue() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("flush receiver");
        let mut receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);

        for _ in 0..256 {
            sender
                .send(response_chunk(vec![1]))
                .await
                .expect("data queue accepts its capacity");
        }
        let error = terminal_error();
        sender
            .send(error.clone())
            .await
            .expect("control messages bypass the data queue");

        match receiver.next().await {
            Some(LinkEvent::Control(message)) => assert_eq!(message, error),
            _ => panic!("the control message is read before the queued data"),
        }
    }

    #[tokio::test]
    async fn flush_acknowledges_only_after_the_receiver_reads_queued_data() {
        let (sender, control_rx, data_rx) = BridgeSender::channel();
        let flush_rx = sender.take_flush_receiver().expect("flush receiver");
        let mut receiver = LinkReceiver::new(control_rx, data_rx, flush_rx);

        sender
            .send(response_chunk(vec![1]))
            .await
            .expect("data is queued before the flush");
        let data = match receiver.next().await {
            Some(LinkEvent::Data(item)) => item,
            _ => panic!("queued data is read first"),
        };

        let flush_sender = sender.clone();
        let flush = tokio::spawn(async move { flush_sender.flush(Duration::from_secs(1)).await });
        let ack = match receiver.next().await {
            Some(LinkEvent::Flush(ack)) => ack,
            _ => panic!("the flush marker reaches the receiver"),
        };
        assert!(
            !flush.is_finished(),
            "the flush is pending until the receiver acknowledges it"
        );

        drop(data);
        ack.send(()).expect("flush acknowledgement is accepted");
        assert!(flush.await.expect("flush task ran"));
    }

    #[tokio::test]
    async fn in_memory_request_channel_keeps_the_existing_bounded_capacity() {
        let link = in_memory_link();
        for index in 0..BRIDGE_LINK_BUFFER {
            link.relay_request_tx
                .try_send(BridgeMessage::Ping)
                .unwrap_or_else(|_| panic!("request channel slot {index} is available"));
        }
        assert!(
            link.relay_request_tx.try_send(BridgeMessage::Ping).is_err(),
            "the in-memory request channel is bounded at the relay send buffer"
        );
    }
}
