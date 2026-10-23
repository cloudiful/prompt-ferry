use std::{fmt, sync::Arc, time::Duration};

use crate::protocol::{BridgeMessage, McpResponseChunk, ResponseChunk};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

const BRIDGE_OUTBOUND_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct BridgeSender {
    data_tx: mpsc::Sender<BridgeData>,
    control_tx: mpsc::UnboundedSender<BridgeMessage>,
    flush_tx: mpsc::UnboundedSender<oneshot::Sender<()>>,
    flush_rx: Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<oneshot::Sender<()>>>>>,
    queued_bytes: Arc<Semaphore>,
}

pub(super) struct BridgeData {
    pub(super) message: BridgeMessage,
    #[allow(dead_code)]
    permit: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub(super) enum BridgeSendError {
    Closed,
    TooLarge,
    Encoding(String),
}

impl BridgeSendError {
    pub(super) fn diagnostic_reason(&self) -> &'static str {
        match self {
            Self::Closed => "relay_bridge_closed",
            Self::TooLarge => "relay_bridge_message_too_large",
            Self::Encoding(_) => "relay_bridge_encode_error",
        }
    }
}

impl fmt::Display for BridgeSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("relay bridge channel closed"),
            Self::TooLarge => {
                formatter.write_str("relay bridge message exceeds the outbound byte budget")
            }
            Self::Encoding(error) => write!(formatter, "failed to encode bridge message: {error}"),
        }
    }
}

impl std::error::Error for BridgeSendError {}

impl BridgeSender {
    pub(super) fn channel() -> (
        Self,
        mpsc::UnboundedReceiver<BridgeMessage>,
        mpsc::Receiver<BridgeData>,
    ) {
        Self::channel_with_byte_budget(BRIDGE_OUTBOUND_MAX_BYTES)
    }

    pub(super) fn channel_with_byte_budget(
        byte_budget: usize,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<BridgeMessage>,
        mpsc::Receiver<BridgeData>,
    ) {
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (data_tx, data_rx) = mpsc::channel(256);
        let (flush_tx, flush_rx) = mpsc::unbounded_channel();
        (
            Self {
                data_tx,
                control_tx,
                flush_tx,
                flush_rx: Arc::new(std::sync::Mutex::new(Some(flush_rx))),
                queued_bytes: Arc::new(Semaphore::new(byte_budget)),
            },
            control_rx,
            data_rx,
        )
    }

    /// Hand the single ordered-flush request receiver to the session writer.
    ///
    /// The writer drains both outbound queues and flushes its websocket sink
    /// before acknowledging; the caller drops every producer first so nothing
    /// can be queued behind the marker.
    pub(super) fn take_flush_receiver(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<oneshot::Sender<()>>> {
        self.flush_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// Ask the writer to flush everything queued before this call.
    ///
    /// Returns `false` when the writer is gone, the socket write fails, or the
    /// bounded allowance expires; a closed or failing socket never reports a
    /// successful flush.
    pub(super) async fn flush(&self, allowance: Duration) -> bool {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.flush_tx.send(ack_tx).is_err() {
            return false;
        }
        matches!(tokio::time::timeout(allowance, ack_rx).await, Ok(Ok(())))
    }

    #[cfg(test)]
    pub(super) fn test_sender() -> Self {
        Self::channel().0
    }

    pub(super) async fn send(&self, message: BridgeMessage) -> Result<(), BridgeSendError> {
        if is_control_message(&message) {
            return self
                .control_tx
                .send(message)
                .map_err(|_| BridgeSendError::Closed);
        }
        let prepared = prepare_bridge_messages(message)?;
        for (message, bytes) in prepared {
            self.send_data_message(message, bytes).await?;
        }
        Ok(())
    }

    async fn send_data_message(
        &self,
        message: BridgeMessage,
        bytes: usize,
    ) -> Result<(), BridgeSendError> {
        let permit = self
            .queued_bytes
            .clone()
            .acquire_many_owned(bytes as u32)
            .await
            .map_err(|_| BridgeSendError::Closed)?;
        self.data_tx
            .send(BridgeData { message, permit })
            .await
            .map_err(|_| BridgeSendError::Closed)
    }
}

fn prepare_bridge_messages(
    message: BridgeMessage,
) -> Result<Vec<(BridgeMessage, usize)>, BridgeSendError> {
    let bytes = crate::bridge_wire::encode_message(&message)
        .map_err(|error| BridgeSendError::Encoding(error.to_string()))?
        .len();
    if bytes <= BRIDGE_OUTBOUND_MAX_BYTES {
        return Ok(vec![(message, bytes)]);
    }
    let mut pending: std::collections::VecDeque<BridgeMessage> = fragment_chunk_message(message)
        .ok_or(BridgeSendError::TooLarge)?
        .into();
    let mut prepared = Vec::new();
    while let Some(fragment) = pending.pop_front() {
        let bytes = crate::bridge_wire::encode_message(&fragment)
            .map_err(|error| BridgeSendError::Encoding(error.to_string()))?
            .len();
        if bytes <= BRIDGE_OUTBOUND_MAX_BYTES {
            prepared.push((fragment, bytes));
            continue;
        }
        // zstd can expand an incompressible payload just past the budget; requesting
        // more permits than the semaphore capacity would wait forever, so halve the
        // fragment until its encoded size fits.
        let (head, tail) = halve_chunk_message(fragment).ok_or(BridgeSendError::TooLarge)?;
        pending.push_front(tail);
        pending.push_front(head);
    }
    Ok(prepared)
}

fn is_control_message(message: &BridgeMessage) -> bool {
    // Terminal errors must bypass a saturated data queue so the relay can close the request.
    matches!(
        message,
        BridgeMessage::Pong | BridgeMessage::Ping | BridgeMessage::ResponseError(_)
    )
}

fn fragment_chunk_message(message: BridgeMessage) -> Option<Vec<BridgeMessage>> {
    let (request_id, mut data, is_mcp) = take_chunk_parts(message)?;
    let fragment_limit = fragment_data_limit(&request_id, is_mcp)?;
    let mut fragments = Vec::new();
    while !data.is_empty() {
        let mut head = std::mem::take(&mut data);
        data = head.split_off(head.len().min(fragment_limit));
        fragments.push(build_chunk_message(request_id.clone(), head, is_mcp));
    }
    Some(fragments)
}

fn fragment_data_limit(request_id: &str, is_mcp: bool) -> Option<usize> {
    let empty = build_chunk_message(request_id.to_string(), Vec::new(), is_mcp);
    let overhead = crate::bridge_wire::encode_message(&empty).ok()?.len();
    let limit = BRIDGE_OUTBOUND_MAX_BYTES.saturating_sub(overhead);
    (limit > 0).then_some(limit)
}

fn halve_chunk_message(message: BridgeMessage) -> Option<(BridgeMessage, BridgeMessage)> {
    let (request_id, mut data, is_mcp) = take_chunk_parts(message)?;
    let mut head = std::mem::take(&mut data);
    data = head.split_off(head.len() / 2);
    Some((
        build_chunk_message(request_id.clone(), head, is_mcp),
        build_chunk_message(request_id, data, is_mcp),
    ))
}

fn take_chunk_parts(message: BridgeMessage) -> Option<(String, Vec<u8>, bool)> {
    match message {
        BridgeMessage::ResponseChunk(chunk) => Some((chunk.request_id, chunk.data, false)),
        BridgeMessage::McpResponseChunk(chunk) => Some((chunk.request_id, chunk.data, true)),
        _ => None,
    }
}

fn build_chunk_message(request_id: String, data: Vec<u8>, is_mcp: bool) -> BridgeMessage {
    if is_mcp {
        BridgeMessage::McpResponseChunk(McpResponseChunk { request_id, data })
    } else {
        BridgeMessage::ResponseChunk(ResponseChunk { request_id, data })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::BridgeSender;
    use crate::protocol::{BridgeMessage, ResponseChunk};

    fn response_chunk() -> BridgeMessage {
        BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "request-1".to_string(),
            data: vec![1],
        })
    }

    #[tokio::test]
    async fn flush_acknowledges_after_the_writer_drains_queued_data() {
        let (sender, _control_rx, mut data_rx) = BridgeSender::channel();
        let mut flush_rx = sender.take_flush_receiver().expect("writer receiver");
        sender.send(response_chunk()).await.expect("queued data");

        let writer = tokio::spawn(async move {
            let item = data_rx.recv().await.expect("queued data reached writer");
            drop(item);
            let ack = flush_rx.recv().await.expect("flush request reached writer");
            ack.send(()).expect("flush acknowledgement accepted");
        });

        assert!(sender.flush(Duration::from_secs(1)).await);
        writer.await.expect("writer task ran");
    }

    #[tokio::test]
    async fn flush_without_a_writer_reports_failure() {
        let (sender, _control_rx, _data_rx) = BridgeSender::channel();
        drop(sender.take_flush_receiver());

        assert!(!sender.flush(Duration::from_millis(50)).await);
    }

    #[tokio::test]
    async fn flush_reports_failure_when_the_writer_drops_the_acknowledgement() {
        let (sender, _control_rx, _data_rx) = BridgeSender::channel();
        let mut flush_rx = sender.take_flush_receiver().expect("writer receiver");
        let writer = tokio::spawn(async move {
            let ack = flush_rx.recv().await.expect("flush request reached writer");
            drop(ack);
        });

        assert!(!sender.flush(Duration::from_secs(1)).await);
        writer.await.expect("writer task ran");
    }
}
