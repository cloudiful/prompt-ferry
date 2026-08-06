use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;
use crate::config::BridgeEncryptionMode;
use crate::protocol::{BridgeRequestChunk, BridgeRequestStart, ResponseStart};

fn key(byte: u8) -> String {
    STANDARD.encode([byte; KEY_BYTES])
}

fn nonces() -> ([u8; HANDSHAKE_NONCE_BYTES], [u8; HANDSHAKE_NONCE_BYTES]) {
    ([1_u8; HANDSHAKE_NONCE_BYTES], [2_u8; HANDSHAKE_NONCE_BYTES])
}

#[test]
fn validates_required_key() {
    validate_settings("worker", BridgeEncryptionMode::Off, "").unwrap();
    assert!(validate_settings("worker", BridgeEncryptionMode::Required, "").is_err());
    assert!(validate_settings("worker", BridgeEncryptionMode::Required, "bad").is_err());
    assert!(
        validate_settings(
            "worker",
            BridgeEncryptionMode::Required,
            &STANDARD.encode([1_u8; 8])
        )
        .is_err()
    );
    validate_settings("worker", BridgeEncryptionMode::Required, &key(7)).unwrap();
}

#[test]
fn same_key_and_nonces_round_trip() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();

    let message = BridgeMessage::RequestStart(BridgeRequestStart {
        request_id: "req".to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        request_deadline_unix_ms: 456,
        user_id: None,
        route_id: None,
        client_key_hash: None,
        request_user_agent: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
    });

    let encrypted = worker.encrypt_message(&message).unwrap();
    assert_eq!(relay.decrypt_message(&encrypted).unwrap(), message);

    let response = BridgeMessage::ResponseStart(ResponseStart {
        request_id: "req".to_string(),
        status: 200,
        content_type: Some("application/json".to_string()),
        headers: vec![("cache-control".to_string(), "no-store".to_string())],
    });
    let encrypted = relay.encrypt_message(&response).unwrap();
    assert_eq!(worker.decrypt_message(&encrypted).unwrap(), response);
}

#[test]
fn large_messages_round_trip_with_compression_and_encryption() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();

    let message = BridgeMessage::RequestChunk(BridgeRequestChunk {
        request_id: "req".to_string(),
        data: vec![b'x'; bridge_wire::BRIDGE_COMPRESSION_THRESHOLD_BYTES + 4096],
    });

    let encrypted = worker.encrypt_message(&message).unwrap();
    assert_eq!(encrypted[0], FRAME_VERSION);
    assert_eq!(relay.decrypt_message(&encrypted).unwrap(), message);
}

#[test]
fn different_key_fails() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(8),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();
    let encrypted = worker.encrypt_message(&BridgeMessage::Ping).unwrap();
    assert!(relay.decrypt_message(&encrypted).is_err());
}

#[test]
fn tampered_ciphertext_fails() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();
    let encrypted = worker.encrypt_message(&BridgeMessage::Ping).unwrap();
    let mut tampered = encrypted.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    assert!(relay.decrypt_message(&tampered).is_err());
}

#[test]
fn replayed_sequence_fails() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();
    let encrypted = worker.encrypt_message(&BridgeMessage::Ping).unwrap();
    relay.decrypt_message(&encrypted).unwrap();
    assert!(relay.decrypt_message(&encrypted).is_err());
}

#[test]
fn invalid_frame_version_fails() {
    let (worker_nonce, relay_nonce) = nonces();
    let mut worker = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )
    .unwrap();
    let mut relay = BridgeCipher::new(
        &key(7),
        &worker_nonce,
        &relay_nonce,
        Direction::RelayToWorker,
        Direction::WorkerToRelay,
    )
    .unwrap();
    let mut encrypted = worker.encrypt_message(&BridgeMessage::Ping).unwrap();
    encrypted[0] = FRAME_VERSION + 1;
    assert!(relay.decrypt_message(&encrypted).is_err());
}
