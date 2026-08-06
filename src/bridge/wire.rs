pub mod payload;

use std::io::Cursor;

use anyhow::{Context, anyhow};

use crate::protocol::BridgeMessage;

/// Current plaintext wire envelope version. Bump when the payload schema
/// changes; peers reject envelopes with other versions.
pub const BRIDGE_WIRE_VERSION: u8 = 4;
pub const PUBLIC_API_BODY_LIMIT_BYTES: usize = 256 * 1024 * 1024;
pub const BRIDGE_WS_MAX_MESSAGE_BYTES: usize = PUBLIC_API_BODY_LIMIT_BYTES + 1024 * 1024;
pub const BRIDGE_WS_MAX_FRAME_BYTES: usize = BRIDGE_WS_MAX_MESSAGE_BYTES;
pub const BRIDGE_COMPRESSION_THRESHOLD_BYTES: usize = 32 * 1024;

const COMPRESSION_NONE: u8 = 0;
const COMPRESSION_ZSTD: u8 = 1;
const ZSTD_LEVEL: i32 = 3;
const HEADER_BYTES: usize = 2;

pub fn encode_message(message: &BridgeMessage) -> anyhow::Result<Vec<u8>> {
    encode_message_with_limit(
        message,
        BRIDGE_WS_MAX_MESSAGE_BYTES,
        BRIDGE_COMPRESSION_THRESHOLD_BYTES,
    )
}

pub fn decode_message(bytes: &[u8]) -> anyhow::Result<BridgeMessage> {
    if bytes.len() < HEADER_BYTES {
        return Err(anyhow!(
            "bridge wire message too short ({} bytes)",
            bytes.len()
        ));
    }
    let version = bytes[0];
    if version != BRIDGE_WIRE_VERSION {
        return Err(anyhow!(
            "unsupported bridge wire version {version}, expected {BRIDGE_WIRE_VERSION}"
        ));
    }
    let compression = bytes[1];
    let payload = &bytes[HEADER_BYTES..];
    let decoded = match compression {
        COMPRESSION_NONE => payload.to_vec(),
        COMPRESSION_ZSTD => zstd::stream::decode_all(Cursor::new(payload)).with_context(|| {
            format!(
                "failed to decompress bridge payload (frame {} bytes, compression {compression})",
                bytes.len()
            )
        })?,
        other => return Err(anyhow!("unsupported bridge wire compression {other}")),
    };
    payload::decode_payload(&decoded).with_context(|| {
        format!(
            "failed to decode bridge payload (frame {} bytes, compression {compression})",
            bytes.len()
        )
    })
}

fn encode_message_with_limit(
    message: &BridgeMessage,
    max_message_bytes: usize,
    compression_threshold_bytes: usize,
) -> anyhow::Result<Vec<u8>> {
    let payload = payload::encode_payload(message)?;
    let (compression, payload) = if payload.len() >= compression_threshold_bytes {
        (
            COMPRESSION_ZSTD,
            zstd::stream::encode_all(Cursor::new(&payload), ZSTD_LEVEL)
                .context("failed to compress bridge payload")?,
        )
    } else {
        (COMPRESSION_NONE, payload)
    };
    let mut wire = Vec::with_capacity(HEADER_BYTES + payload.len());
    wire.push(BRIDGE_WIRE_VERSION);
    wire.push(compression);
    wire.extend_from_slice(&payload);
    if wire.len() > max_message_bytes {
        return Err(anyhow!(
            "bridge wire message exceeds limit: {} > {}",
            wire.len(),
            max_message_bytes
        ));
    }
    Ok(wire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BridgeRequestStart, ResponseChunk};

    #[test]
    fn messages_round_trip_with_bincode_wire() {
        let message = BridgeMessage::RequestStart(BridgeRequestStart {
            request_id: "req".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            request_deadline_unix_ms: 456,
            user_id: Some(1),
            route_id: Some("route".to_string()),
            client_key_hash: Some("hash".to_string()),
            request_user_agent: Some("codex/1.0".to_string()),
            http_request_content_encoding: Some("gzip".to_string()),
            http_request_compressed: true,
            http_request_compressed_bytes: Some(1024),
        });
        let encoded = encode_message(&message).unwrap();
        assert_eq!(decode_message(&encoded).unwrap(), message);
    }

    #[test]
    fn small_messages_stay_uncompressed() {
        let message = BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "req".to_string(),
            data: vec![b'a'; BRIDGE_COMPRESSION_THRESHOLD_BYTES / 2],
        });
        let encoded = encode_message(&message).unwrap();
        assert_eq!(encoded[0], BRIDGE_WIRE_VERSION);
        assert_eq!(encoded[1], COMPRESSION_NONE);
        assert_eq!(decode_message(&encoded).unwrap(), message);
    }

    #[test]
    fn large_messages_are_compressed() {
        let message = BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "req".to_string(),
            data: vec![b'a'; BRIDGE_COMPRESSION_THRESHOLD_BYTES + 1024],
        });
        let encoded = encode_message(&message).unwrap();
        assert_eq!(encoded[0], BRIDGE_WIRE_VERSION);
        assert_eq!(encoded[1], COMPRESSION_ZSTD);
        assert_eq!(decode_message(&encoded).unwrap(), message);
    }

    #[test]
    fn invalid_wire_version_fails() {
        let err = decode_message(&[BRIDGE_WIRE_VERSION + 1, COMPRESSION_NONE]).unwrap_err();
        assert!(err.to_string().contains("unsupported bridge wire version"));
    }

    #[test]
    fn invalid_compression_flag_fails() {
        let err = decode_message(&[BRIDGE_WIRE_VERSION, 9]).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported bridge wire compression")
        );
    }

    #[test]
    fn damaged_compressed_payload_fails() {
        let message = BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "req".to_string(),
            data: vec![b'b'; BRIDGE_COMPRESSION_THRESHOLD_BYTES + 1024],
        });
        let mut encoded = encode_message(&message).unwrap();
        encoded.truncate(encoded.len().saturating_sub(8));
        let err = decode_message(&encoded).unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to decompress bridge payload")
        );
    }

    #[test]
    fn oversized_message_is_rejected() {
        let message = BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "req".to_string(),
            data: vec![0_u8; 32],
        });
        let err = encode_message_with_limit(&message, 8, usize::MAX).unwrap_err();
        assert!(
            err.to_string()
                .contains("bridge wire message exceeds limit")
        );
    }
}
