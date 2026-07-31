use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnqueueResult {
    Enqueued,
    BytesLimit,
    Full,
    Closed,
}

pub(crate) fn try_enqueue<T>(
    sender: &mpsc::Sender<T>,
    queued_bytes: &mut usize,
    item: T,
    item_bytes: usize,
    max_bytes: usize,
) -> EnqueueResult {
    if sender.is_closed() {
        return EnqueueResult::Closed;
    }

    let next_bytes = queued_bytes
        .checked_add(item_bytes)
        .filter(|bytes| *bytes <= max_bytes);
    let Some(next_bytes) = next_bytes else {
        return EnqueueResult::BytesLimit;
    };

    match sender.try_send(item) {
        Ok(()) => {
            *queued_bytes = next_bytes;
            EnqueueResult::Enqueued
        }
        Err(mpsc::error::TrySendError::Full(_)) => EnqueueResult::Full,
        Err(mpsc::error::TrySendError::Closed(_)) => EnqueueResult::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::{EnqueueResult, try_enqueue};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn distinguishes_full_from_closed() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut queued_bytes = 0;

        assert_eq!(
            try_enqueue(&sender, &mut queued_bytes, vec![1], 1, 10),
            EnqueueResult::Enqueued
        );
        assert_eq!(queued_bytes, 1);
        assert_eq!(
            try_enqueue(&sender, &mut queued_bytes, vec![2], 1, 10),
            EnqueueResult::Full
        );
        assert_eq!(queued_bytes, 1);

        receiver.recv().await;
        drop(receiver);
        assert_eq!(
            try_enqueue(&sender, &mut queued_bytes, vec![3], 1, 10),
            EnqueueResult::Closed
        );
    }

    #[tokio::test]
    async fn enforces_the_byte_limit_before_sending() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut queued_bytes = 2;

        assert_eq!(
            try_enqueue(&sender, &mut queued_bytes, vec![1], 3, 4),
            EnqueueResult::BytesLimit
        );
        assert_eq!(queued_bytes, 2);
        assert!(receiver.try_recv().is_err());
    }
}
