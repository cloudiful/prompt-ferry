use crate::protocol::{
    ApprovalPending, BridgeMessage, BridgeRequestCancel, BridgeRequestChunk, BridgeRequestEnd,
    BridgeRequestStart, ClientRoute, ConfigSnapshot, McpRequestCancel, McpRequestChunk,
    McpRequestEnd, McpRequestStart, McpResponseChunk, McpResponseEnd, McpResponseStart,
    RealtimeClientEventMessage, RealtimeServerEventMessage, RealtimeSessionClose,
    RealtimeSessionStart, RelayIpPolicy, ResponseChunk, ResponseEnd, ResponseError, ResponseStart,
};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub const BRIDGE_WIRE_VERSION: u8 = 3;
pub const PUBLIC_API_BODY_LIMIT_BYTES: usize = 256 * 1024 * 1024;
pub const BRIDGE_WS_MAX_MESSAGE_BYTES: usize = PUBLIC_API_BODY_LIMIT_BYTES + 1024 * 1024;
pub const BRIDGE_WS_MAX_FRAME_BYTES: usize = BRIDGE_WS_MAX_MESSAGE_BYTES;
pub const BRIDGE_COMPRESSION_THRESHOLD_BYTES: usize = 32 * 1024;

const COMPRESSION_NONE: u8 = 0;
const COMPRESSION_ZSTD: u8 = 1;
const ZSTD_LEVEL: i32 = 3;
const HEADER_BYTES: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum BridgeWirePayload {
    RequestStart(BridgeRequestStart),
    RequestChunk(BridgeRequestChunk),
    RequestEnd(BridgeRequestEnd),
    RequestCancel(BridgeRequestCancel),
    ApprovalPending(ApprovalPending),
    RealtimeSessionStart(RealtimeSessionStart),
    RealtimeClientEvent(RealtimeClientEventMessage),
    RealtimeServerEvent(RealtimeServerEventMessage),
    RealtimeSessionClose(RealtimeSessionClose),
    McpRequestStart(McpRequestStart),
    McpRequestChunk(McpRequestChunk),
    McpRequestEnd(McpRequestEnd),
    McpRequestCancel(McpRequestCancel),
    McpResponseStart(McpResponseStart),
    McpResponseChunk(McpResponseChunk),
    McpResponseEnd(McpResponseEnd),
    ConfigSnapshot(BridgeWireConfigSnapshot),
    ResponseStart(ResponseStart),
    ResponseChunk(ResponseChunk),
    ResponseEnd(ResponseEnd),
    ResponseError(ResponseError),
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BridgeWireConfigSnapshot {
    version: i64,
    keys: Vec<ClientRoute>,
    relay_ip_policy: RelayIpPolicy,
}

impl From<&BridgeMessage> for BridgeWirePayload {
    fn from(message: &BridgeMessage) -> Self {
        match message {
            BridgeMessage::RequestStart(value) => Self::RequestStart(value.clone()),
            BridgeMessage::RequestChunk(value) => Self::RequestChunk(value.clone()),
            BridgeMessage::RequestEnd(value) => Self::RequestEnd(value.clone()),
            BridgeMessage::RequestCancel(value) => Self::RequestCancel(value.clone()),
            BridgeMessage::ApprovalPending(value) => Self::ApprovalPending(value.clone()),
            BridgeMessage::RealtimeSessionStart(value) => Self::RealtimeSessionStart(value.clone()),
            BridgeMessage::RealtimeClientEvent(value) => Self::RealtimeClientEvent(value.clone()),
            BridgeMessage::RealtimeServerEvent(value) => Self::RealtimeServerEvent(value.clone()),
            BridgeMessage::RealtimeSessionClose(value) => Self::RealtimeSessionClose(value.clone()),
            BridgeMessage::McpRequestStart(value) => Self::McpRequestStart(value.clone()),
            BridgeMessage::McpRequestChunk(value) => Self::McpRequestChunk(value.clone()),
            BridgeMessage::McpRequestEnd(value) => Self::McpRequestEnd(value.clone()),
            BridgeMessage::McpRequestCancel(value) => Self::McpRequestCancel(value.clone()),
            BridgeMessage::McpResponseStart(value) => Self::McpResponseStart(value.clone()),
            BridgeMessage::McpResponseChunk(value) => Self::McpResponseChunk(value.clone()),
            BridgeMessage::McpResponseEnd(value) => Self::McpResponseEnd(value.clone()),
            BridgeMessage::ConfigSnapshot(value) => Self::ConfigSnapshot(value.into()),
            BridgeMessage::ResponseStart(value) => Self::ResponseStart(value.clone()),
            BridgeMessage::ResponseChunk(value) => Self::ResponseChunk(value.clone()),
            BridgeMessage::ResponseEnd(value) => Self::ResponseEnd(value.clone()),
            BridgeMessage::ResponseError(value) => Self::ResponseError(value.clone()),
            BridgeMessage::Ping => Self::Ping,
            BridgeMessage::Pong => Self::Pong,
        }
    }
}

impl From<&ConfigSnapshot> for BridgeWireConfigSnapshot {
    fn from(value: &ConfigSnapshot) -> Self {
        Self {
            version: value.version,
            keys: value.keys.clone(),
            relay_ip_policy: value.relay_ip_policy.clone(),
        }
    }
}

impl From<BridgeWireConfigSnapshot> for ConfigSnapshot {
    fn from(value: BridgeWireConfigSnapshot) -> Self {
        Self {
            version: value.version,
            keys: value.keys,
            relay_ip_policy: value.relay_ip_policy,
        }
    }
}

impl BridgeWirePayload {
    fn into_bridge_message(self) -> BridgeMessage {
        match self {
            Self::RequestStart(value) => BridgeMessage::RequestStart(value),
            Self::RequestChunk(value) => BridgeMessage::RequestChunk(value),
            Self::RequestEnd(value) => BridgeMessage::RequestEnd(value),
            Self::RequestCancel(value) => BridgeMessage::RequestCancel(value),
            Self::ApprovalPending(value) => BridgeMessage::ApprovalPending(value),
            Self::RealtimeSessionStart(value) => BridgeMessage::RealtimeSessionStart(value),
            Self::RealtimeClientEvent(value) => BridgeMessage::RealtimeClientEvent(value),
            Self::RealtimeServerEvent(value) => BridgeMessage::RealtimeServerEvent(value),
            Self::RealtimeSessionClose(value) => BridgeMessage::RealtimeSessionClose(value),
            Self::McpRequestStart(value) => BridgeMessage::McpRequestStart(value),
            Self::McpRequestChunk(value) => BridgeMessage::McpRequestChunk(value),
            Self::McpRequestEnd(value) => BridgeMessage::McpRequestEnd(value),
            Self::McpRequestCancel(value) => BridgeMessage::McpRequestCancel(value),
            Self::McpResponseStart(value) => BridgeMessage::McpResponseStart(value),
            Self::McpResponseChunk(value) => BridgeMessage::McpResponseChunk(value),
            Self::McpResponseEnd(value) => BridgeMessage::McpResponseEnd(value),
            Self::ConfigSnapshot(value) => BridgeMessage::ConfigSnapshot(value.into()),
            Self::ResponseStart(value) => BridgeMessage::ResponseStart(value),
            Self::ResponseChunk(value) => BridgeMessage::ResponseChunk(value),
            Self::ResponseEnd(value) => BridgeMessage::ResponseEnd(value),
            Self::ResponseError(value) => BridgeMessage::ResponseError(value),
            Self::Ping => BridgeMessage::Ping,
            Self::Pong => BridgeMessage::Pong,
        }
    }
}

pub fn encode_message(message: &BridgeMessage) -> anyhow::Result<Vec<u8>> {
    encode_message_with_limit(
        message,
        BRIDGE_WS_MAX_MESSAGE_BYTES,
        BRIDGE_COMPRESSION_THRESHOLD_BYTES,
    )
}

pub fn decode_message(bytes: &[u8]) -> anyhow::Result<BridgeMessage> {
    if bytes.len() < HEADER_BYTES {
        return Err(anyhow!("bridge wire message too short"));
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
        COMPRESSION_ZSTD => zstd::stream::decode_all(Cursor::new(payload))
            .context("failed to decompress bridge payload")?,
        other => return Err(anyhow!("unsupported bridge wire compression {other}")),
    };
    let payload: BridgeWirePayload =
        bincode::deserialize(&decoded).context("failed to decode bridge payload")?;
    Ok(payload.into_bridge_message())
}

fn encode_message_with_limit(
    message: &BridgeMessage,
    max_message_bytes: usize,
    compression_threshold_bytes: usize,
) -> anyhow::Result<Vec<u8>> {
    let payload = bincode::serialize(&BridgeWirePayload::from(message))
        .context("failed to encode bridge payload")?;
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
    use crate::protocol::{
        BridgeRequestChunk, BridgeRequestStart, ConfigSnapshot, RelayIpPolicy, ResponseChunk,
    };

    #[test]
    fn bridge_message_round_trips_with_bincode_wire() {
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
    fn request_chunk_round_trips_with_bincode_wire() {
        let message = BridgeMessage::RequestChunk(BridgeRequestChunk {
            request_id: "req".to_string(),
            data: vec![b'a'; BRIDGE_COMPRESSION_THRESHOLD_BYTES + 1024],
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
    fn config_snapshot_round_trips_with_binary_payload() {
        let message = BridgeMessage::ConfigSnapshot(ConfigSnapshot {
            version: 7,
            keys: vec![crate::protocol::ClientRoute {
                key_hash: "hash".to_string(),
                key_prefix: "pref".to_string(),
                user_id: 42,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: RelayIpPolicy {
                allowed_cidrs: vec!["10.0.0.0/8".to_string()],
                trusted_proxy_cidrs: vec!["192.168.0.0/16".to_string()],
            },
        });
        let encoded = encode_message(&message).unwrap();
        assert_eq!(decode_message(&encoded).unwrap(), message);
    }

    #[test]
    fn ping_round_trips_with_binary_payload() {
        let encoded = encode_message(&BridgeMessage::Ping).unwrap();
        assert_eq!(decode_message(&encoded).unwrap(), BridgeMessage::Ping);
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
