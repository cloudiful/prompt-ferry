use std::time::Duration;

use super::bridge::{BridgeSendError, BridgeSender};
use crate::protocol::{BridgeMessage, McpResponseChunk, ResponseChunk, ResponseError};

const FRAGMENT_TEST_BYTES: usize = 16 * 1024 * 1024 + 4096;

fn response_chunk() -> BridgeMessage {
    BridgeMessage::ResponseChunk(ResponseChunk {
        request_id: "request-1".to_string(),
        data: vec![1],
    })
}

fn response_chunk_with_data(data: Vec<u8>) -> BridgeMessage {
    BridgeMessage::ResponseChunk(ResponseChunk {
        request_id: "request-1".to_string(),
        data,
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

fn pseudo_random_byte(index: usize) -> u8 {
    let mut state = index as u64 ^ 0x9e37_79b9_7f4a_7c15;
    state = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    state = (state ^ (state >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    state ^= state >> 31;
    (state >> 56) as u8
}

fn incompressible_bytes(len: usize) -> Vec<u8> {
    (0..len).map(pseudo_random_byte).collect()
}

#[tokio::test]
async fn distinguishes_closed_data_queue() {
    let (sender, _control_rx, data_rx) = BridgeSender::channel();
    drop(data_rx);

    assert!(matches!(
        sender.send(response_chunk()).await,
        Err(BridgeSendError::Closed)
    ));
}

#[tokio::test]
async fn send_waits_for_queue_capacity_instead_of_failing_full() {
    let (sender, _control_rx, mut data_rx) = BridgeSender::channel();
    for _ in 0..256 {
        sender
            .send(response_chunk())
            .await
            .expect("queue has capacity");
    }

    let pending_sender = sender.clone();
    let pending = tokio::spawn(async move { pending_sender.send(response_chunk()).await });

    let mut received = 0usize;
    while received < 257 {
        let item = data_rx.recv().await.expect("writer drains messages");
        drop(item);
        received += 1;
    }
    pending
        .await
        .expect("pending send task ran")
        .expect("send succeeds after the writer consumes a slot");
}

#[tokio::test]
async fn byte_budget_blocks_then_releases_when_writer_consumes() {
    let (sender, _control_rx, mut data_rx) = BridgeSender::channel_with_byte_budget(4096);
    sender
        .send(response_chunk_with_data(vec![0u8; 3000]))
        .await
        .expect("first chunk fits the budget");

    let pending_sender = sender.clone();
    let pending = tokio::spawn(async move {
        pending_sender
            .send(response_chunk_with_data(vec![0u8; 3000]))
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let item = data_rx.recv().await.expect("writer consumed first chunk");
    drop(item);
    pending
        .await
        .expect("pending send task ran")
        .expect("second chunk proceeds once permits are released");
}

#[tokio::test]
async fn cancelling_a_waiting_send_does_not_leak_permits() {
    let (sender, _control_rx, mut data_rx) = BridgeSender::channel_with_byte_budget(4096);
    sender
        .send(response_chunk_with_data(vec![0u8; 3000]))
        .await
        .expect("first chunk fits the budget");

    let pending_sender = sender.clone();
    let pending = tokio::spawn(async move {
        pending_sender
            .send(response_chunk_with_data(vec![0u8; 3000]))
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    pending.abort();

    let item = data_rx.recv().await.expect("writer consumed first chunk");
    drop(item);
    sender
        .send(response_chunk_with_data(vec![0u8; 3000]))
        .await
        .expect("full budget is still available after cancelling the waiter");
}

#[tokio::test]
async fn terminal_errors_bypass_full_data_queue() {
    let (sender, mut control_rx, _data_rx) = BridgeSender::channel();
    for _ in 0..256 {
        sender
            .send(response_chunk())
            .await
            .expect("queue has capacity");
    }

    let error = terminal_error();
    sender
        .send(error.clone())
        .await
        .expect("terminal errors are prioritized");

    assert_eq!(control_rx.try_recv(), Ok(error));
}

#[tokio::test]
async fn oversized_response_chunk_is_fragmented_and_reassembles_identically() {
    let (sender, _control_rx, mut data_rx) = BridgeSender::channel();
    let expected = incompressible_bytes(FRAGMENT_TEST_BYTES);
    let drain = tokio::spawn(async move {
        let mut reassembled = Vec::new();
        while let Some(item) = data_rx.recv().await {
            match item.message {
                BridgeMessage::ResponseChunk(chunk) => reassembled.extend_from_slice(&chunk.data),
                other => panic!("unexpected bridge message: {other:?}"),
            }
        }
        reassembled
    });

    sender
        .send(response_chunk_with_data(expected.clone()))
        .await
        .expect("oversized chunk is fragmented instead of rejected");
    drop(sender);

    let reassembled = drain.await.expect("drain task ran");
    assert_eq!(reassembled, expected);
}

#[tokio::test]
async fn oversized_mcp_response_chunk_is_fragmented_and_reassembles_identically() {
    let (sender, _control_rx, mut data_rx) = BridgeSender::channel();
    let expected = incompressible_bytes(FRAGMENT_TEST_BYTES);
    let drain = tokio::spawn(async move {
        let mut reassembled = Vec::new();
        while let Some(item) = data_rx.recv().await {
            match item.message {
                BridgeMessage::McpResponseChunk(chunk) => {
                    reassembled.extend_from_slice(&chunk.data)
                }
                other => panic!("unexpected bridge message: {other:?}"),
            }
        }
        reassembled
    });

    sender
        .send(BridgeMessage::McpResponseChunk(McpResponseChunk {
            request_id: "request-1".to_string(),
            data: expected.clone(),
        }))
        .await
        .expect("oversized MCP chunk is fragmented instead of rejected");
    drop(sender);

    let reassembled = drain.await.expect("drain task ran");
    assert_eq!(reassembled, expected);
}

#[tokio::test]
async fn oversized_non_chunk_message_is_rejected_with_too_large() {
    let (sender, _control_rx, _data_rx) = BridgeSender::channel();
    let event_json: String = (0..16 * 1024 * 1024 + 4096)
        .map(|index| char::from(pseudo_random_byte(index)))
        .collect();
    let oversized =
        BridgeMessage::RealtimeServerEvent(crate::protocol::RealtimeServerEventMessage {
            request_id: "request-1".to_string(),
            event_json,
        });
    assert!(matches!(
        sender.send(oversized).await,
        Err(BridgeSendError::TooLarge)
    ));
}
