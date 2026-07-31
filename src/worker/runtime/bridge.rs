use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::protocol::BridgeMessage;
use tokio::sync::mpsc;

const BRIDGE_OUTBOUND_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct BridgeSender {
    data_tx: mpsc::Sender<BridgeData>,
    control_tx: mpsc::UnboundedSender<BridgeMessage>,
    queued_bytes: Arc<AtomicUsize>,
}

pub(super) struct BridgeData {
    pub(super) message: BridgeMessage,
    pub(super) bytes: usize,
}

#[derive(Debug)]
pub(super) enum BridgeSendError {
    Closed,
    Full,
    Encoding(String),
}

impl BridgeSendError {
    pub(super) fn diagnostic_reason(&self) -> &'static str {
        match self {
            Self::Closed => "relay_bridge_closed",
            Self::Full => "relay_bridge_backpressure",
            Self::Encoding(_) => "relay_bridge_encode_error",
        }
    }
}

impl fmt::Display for BridgeSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("relay bridge channel closed"),
            Self::Full => formatter.write_str("relay bridge outbound queue is full"),
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
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (data_tx, data_rx) = mpsc::channel(256);
        (
            Self {
                data_tx,
                control_tx,
                queued_bytes: Arc::new(AtomicUsize::new(0)),
            },
            control_rx,
            data_rx,
        )
    }

    #[cfg(test)]
    pub(super) fn test_sender() -> Self {
        Self::channel().0
    }

    pub(super) fn send(&self, message: BridgeMessage) -> Result<(), BridgeSendError> {
        if is_control_message(&message) {
            return self
                .control_tx
                .send(message)
                .map_err(|_| BridgeSendError::Closed);
        }
        let bytes = crate::bridge_wire::encode_message(&message)
            .map_err(|error| BridgeSendError::Encoding(error.to_string()))?
            .len();
        let mut current = self.queued_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return Err(BridgeSendError::Full);
            };
            if next > BRIDGE_OUTBOUND_MAX_BYTES {
                return Err(BridgeSendError::Full);
            }
            match self.queued_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        match self.data_tx.try_send(BridgeData { message, bytes }) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                Err(BridgeSendError::Closed)
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                Err(BridgeSendError::Full)
            }
        }
    }

    pub(super) fn release_data(&self, bytes: usize) {
        self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
    }
}

fn is_control_message(message: &BridgeMessage) -> bool {
    // Terminal errors must bypass a saturated data queue so the relay can close the request.
    matches!(
        message,
        BridgeMessage::Pong | BridgeMessage::Ping | BridgeMessage::ResponseError(_)
    )
}

#[cfg(test)]
mod tests {
    use super::{BridgeSendError, BridgeSender};
    use crate::protocol::{BridgeMessage, ResponseChunk, ResponseError};

    fn response_chunk() -> BridgeMessage {
        BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "request-1".to_string(),
            data: vec![1],
        })
    }

    #[test]
    fn distinguishes_closed_data_queue() {
        let (sender, _control_rx, data_rx) = BridgeSender::channel();
        drop(data_rx);

        assert!(matches!(
            sender.send(response_chunk()),
            Err(BridgeSendError::Closed)
        ));
    }

    #[test]
    fn distinguishes_full_data_queue() {
        let (sender, _control_rx, _data_rx) = BridgeSender::channel();
        for _ in 0..256 {
            sender.send(response_chunk()).expect("queue has capacity");
        }

        assert!(matches!(
            sender.send(response_chunk()),
            Err(BridgeSendError::Full)
        ));
    }

    #[test]
    fn terminal_errors_bypass_full_data_queue() {
        let (sender, mut control_rx, _data_rx) = BridgeSender::channel();
        for _ in 0..256 {
            sender.send(response_chunk()).expect("queue has capacity");
        }

        let error = BridgeMessage::ResponseError(ResponseError {
            request_id: "request-1".to_string(),
            status: 502,
            code: "relay_bridge_error".to_string(),
            message: "bridge failed".to_string(),
        });
        sender
            .send(error.clone())
            .expect("terminal errors are prioritized");

        assert_eq!(control_rx.try_recv(), Ok(error));
    }
}
