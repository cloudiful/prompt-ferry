use crate::{
    bridge_crypto::{self, BridgeCipher, Direction},
    bridge_wire,
    protocol::BridgeMessage,
};

use super::super::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use futures::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::time::Duration;
use tracing::warn;

pub(super) async fn perform_worker_handshake(
    state: &AppState,
    worker_id: usize,
    ws_tx: &mut SplitSink<WebSocket, Message>,
    ws_rx: &mut SplitStream<WebSocket>,
) -> Option<(Option<BridgeCipher>, Option<BridgeCipher>)> {
    if !state.config.bridge_encryption_mode.required() {
        return Some((None, None));
    }

    let text = match tokio::time::timeout(Duration::from_secs(10), ws_rx.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text.to_string(),
        Ok(Some(Ok(Message::Binary(bytes)))) => match String::from_utf8(bytes.to_vec()) {
            Ok(text) => text,
            Err(err) => {
                warn!(worker_id, error = %err, "invalid encryption hello utf8");
                return None;
            }
        },
        Ok(Some(Ok(_))) => {
            warn!(worker_id, "worker did not send encryption hello");
            return None;
        }
        Ok(Some(Err(err))) => {
            warn!(worker_id, error = %err, "worker websocket error during encryption hello");
            return None;
        }
        Ok(None) => return None,
        Err(_) => {
            warn!(worker_id, "timed out waiting for worker encryption hello");
            return None;
        }
    };
    let worker_nonce = match bridge_crypto::decode_hello(&text) {
        Ok(nonce) => nonce,
        Err(err) => {
            warn!(worker_id, error = %err, "invalid worker encryption hello");
            return None;
        }
    };
    let relay_nonce = bridge_crypto::random_handshake_nonce();
    let ready = match bridge_crypto::encode_ready(&relay_nonce) {
        Ok(ready) => ready,
        Err(err) => {
            warn!(worker_id, error = %err, "failed to encode encryption ready");
            return None;
        }
    };
    if ws_tx.send(Message::Text(ready.into())).await.is_err() {
        return None;
    }
    let mut write_cipher = match BridgeCipher::new(
        &state.config.bridge_encryption_key,
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    ) {
        Ok(cipher) => cipher,
        Err(err) => {
            warn!(worker_id, error = %err, "failed to initialize bridge encryption writer");
            return None;
        }
    };
    let mut read_cipher = match BridgeCipher::new(
        &state.config.bridge_encryption_key,
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    ) {
        Ok(cipher) => cipher,
        Err(err) => {
            warn!(worker_id, error = %err, "failed to initialize bridge encryption reader");
            return None;
        }
    };
    let bytes = match tokio::time::timeout(Duration::from_secs(10), ws_rx.next()).await {
        Ok(Some(Ok(Message::Binary(bytes)))) => bytes.to_vec(),
        Ok(Some(Ok(Message::Text(_)))) => {
            warn!(
                worker_id,
                "worker sent unexpected text encrypted verification"
            );
            return None;
        }
        Ok(Some(Ok(_))) => {
            warn!(worker_id, "worker did not send encrypted verification");
            return None;
        }
        Ok(Some(Err(err))) => {
            warn!(worker_id, error = %err, "worker websocket error during encrypted verification");
            return None;
        }
        Ok(None) => return None,
        Err(_) => {
            warn!(
                worker_id,
                "timed out waiting for encrypted worker verification"
            );
            return None;
        }
    };
    match read_cipher.decrypt_message(&bytes) {
        Ok(BridgeMessage::Ping) => {}
        Ok(message) => {
            warn!(
                worker_id,
                ?message,
                "unexpected encrypted worker verification message"
            );
            return None;
        }
        Err(err) => {
            warn!(worker_id, error = %err, "failed encrypted worker verification");
            return None;
        }
    }
    let pong = match write_cipher.encrypt_message(&BridgeMessage::Pong) {
        Ok(pong) => pong,
        Err(err) => {
            warn!(worker_id, error = %err, "failed to encode encrypted verification pong");
            return None;
        }
    };
    if ws_tx.send(Message::Binary(pong.into())).await.is_err() {
        return None;
    }
    Some((Some(write_cipher), Some(read_cipher)))
}

pub(super) async fn recv_worker_message(
    worker_id: usize,
    heartbeat_timeout: Duration,
    ws_rx: &mut SplitStream<WebSocket>,
    mut read_cipher: Option<&mut BridgeCipher>,
) -> Option<BridgeMessage> {
    loop {
        let result = match tokio::time::timeout(heartbeat_timeout, ws_rx.next()).await {
            Ok(Some(result)) => result,
            Ok(None) => return None,
            Err(_) => {
                warn!(
                    worker_id,
                    timeout_seconds = heartbeat_timeout.as_secs(),
                    "worker heartbeat timed out"
                );
                return None;
            }
        };

        match result {
            Ok(Message::Text(_)) => {
                warn!(worker_id, "unexpected text worker bridge message");
                return None;
            }
            Ok(Message::Binary(bytes)) => {
                let decoded = if let Some(cipher) = read_cipher.as_deref_mut() {
                    cipher.decrypt_message(&bytes)
                } else {
                    bridge_wire::decode_message(&bytes)
                };
                match decoded {
                    Ok(message) => return Some(message),
                    Err(err) => {
                        warn!(worker_id, error = %err, "failed to decode worker message");
                        return None;
                    }
                }
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => return None,
            Err(err) => {
                warn!(worker_id, error = %err, "worker websocket error");
                return None;
            }
        }
    }
}
