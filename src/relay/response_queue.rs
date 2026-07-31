use std::time::Duration;

use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnqueueResult {
    Enqueued,
    BytesLimit,
    Full,
    Closed,
}

pub(crate) fn reserve_bytes(
    queued_bytes: &mut usize,
    item_bytes: usize,
    max_bytes: usize,
) -> EnqueueResult {
    let Some(next_bytes) = queued_bytes
        .checked_add(item_bytes)
        .filter(|bytes| *bytes <= max_bytes)
    else {
        return EnqueueResult::BytesLimit;
    };
    *queued_bytes = next_bytes;
    EnqueueResult::Enqueued
}

pub(crate) async fn send_with_backpressure<T>(
    sender: &mpsc::Sender<T>,
    item: T,
    timeout: Duration,
) -> EnqueueResult {
    if sender.is_closed() {
        return EnqueueResult::Closed;
    }

    match tokio::time::timeout(timeout, sender.send(item)).await {
        Ok(Ok(())) => EnqueueResult::Enqueued,
        Ok(Err(_)) => EnqueueResult::Closed,
        Err(_) => EnqueueResult::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::{EnqueueResult, reserve_bytes, send_with_backpressure};
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn reports_closed_receiver() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);

        assert_eq!(
            send_with_backpressure(&sender, vec![1], Duration::from_millis(20)).await,
            EnqueueResult::Closed
        );
    }

    #[tokio::test]
    async fn waits_through_a_transient_full_queue() {
        let (sender, mut receiver) = mpsc::channel(1);
        sender.send(vec![1]).await.expect("initial item fits");
        let drain = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            let item = receiver.recv().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            item
        });

        assert_eq!(
            send_with_backpressure(&sender, vec![2], Duration::from_millis(100)).await,
            EnqueueResult::Enqueued
        );
        assert_eq!(drain.await.expect("drain task completes"), Some(vec![1]));
    }

    #[tokio::test]
    async fn times_out_when_queue_stays_full() {
        let (sender, _receiver) = mpsc::channel::<Vec<u8>>(1);
        sender.send(vec![1]).await.expect("initial item fits");

        assert_eq!(
            send_with_backpressure(&sender, vec![2], Duration::from_millis(10)).await,
            EnqueueResult::Full
        );
    }

    #[test]
    fn enforces_the_byte_limit_before_sending() {
        let mut queued_bytes = 2;
        assert_eq!(
            reserve_bytes(&mut queued_bytes, 3, 4),
            EnqueueResult::BytesLimit
        );
        assert_eq!(queued_bytes, 2);
        assert_eq!(
            reserve_bytes(&mut queued_bytes, 2, 4),
            EnqueueResult::Enqueued
        );
        assert_eq!(queued_bytes, 4);
    }
}
