//! Current bridge payload schema, bincode-encoded inside the wire envelope.
//!
//! bincode 1.x serializes every declared field in declaration order and cannot
//! default missing trailing fields, so schema growth changes the wire bytes.
//! When fields are added here, bump [`super::BRIDGE_WIRE_VERSION`] and deploy
//! relays and workers together.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::protocol::{
    ApprovalPending, BridgeMessage, BridgeRequestCancel, BridgeRequestChunk, BridgeRequestEnd,
    BridgeRequestStart, ConfigSnapshot, McpRequestCancel, McpRequestChunk, McpRequestEnd,
    McpRequestStart, McpResponseChunk, McpResponseEnd, McpResponseStart,
    RealtimeClientEventMessage, RealtimeServerEventMessage, RealtimeSessionClose,
    RealtimeSessionStart, ResponseChunk, ResponseEnd, ResponseError, ResponseStart,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum WirePayload {
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
    ConfigSnapshot(ConfigSnapshot),
    ResponseStart(ResponseStart),
    ResponseChunk(ResponseChunk),
    ResponseEnd(ResponseEnd),
    ResponseError(ResponseError),
    Ping,
    Pong,
}

impl From<&BridgeMessage> for WirePayload {
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
            BridgeMessage::ConfigSnapshot(value) => Self::ConfigSnapshot(value.clone()),
            BridgeMessage::ResponseStart(value) => Self::ResponseStart(value.clone()),
            BridgeMessage::ResponseChunk(value) => Self::ResponseChunk(value.clone()),
            BridgeMessage::ResponseEnd(value) => Self::ResponseEnd(value.clone()),
            BridgeMessage::ResponseError(value) => Self::ResponseError(value.clone()),
            BridgeMessage::Ping => Self::Ping,
            BridgeMessage::Pong => Self::Pong,
        }
    }
}

impl WirePayload {
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
            Self::ConfigSnapshot(value) => BridgeMessage::ConfigSnapshot(value),
            Self::ResponseStart(value) => BridgeMessage::ResponseStart(value),
            Self::ResponseChunk(value) => BridgeMessage::ResponseChunk(value),
            Self::ResponseEnd(value) => BridgeMessage::ResponseEnd(value),
            Self::ResponseError(value) => BridgeMessage::ResponseError(value),
            Self::Ping => BridgeMessage::Ping,
            Self::Pong => BridgeMessage::Pong,
        }
    }
}

pub(crate) fn encode_payload(message: &BridgeMessage) -> anyhow::Result<Vec<u8>> {
    bincode::serialize(&WirePayload::from(message)).context("failed to encode bridge payload")
}

pub(crate) fn decode_payload(bytes: &[u8]) -> anyhow::Result<BridgeMessage> {
    let payload: WirePayload =
        bincode::deserialize(bytes).context("failed to decode bridge payload")?;
    Ok(payload.into_bridge_message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{McpRequestCancel, RealtimeSessionClose};

    fn round_trips(message: BridgeMessage) {
        let encoded = encode_payload(&message).unwrap();
        assert_eq!(decode_payload(&encoded).unwrap(), message);
    }

    #[test]
    fn cancel_messages_preserve_response_started() {
        round_trips(BridgeMessage::RequestCancel(BridgeRequestCancel {
            request_id: "req".to_string(),
            reason: "downstream_closed".to_string(),
            response_started: true,
        }));
        round_trips(BridgeMessage::RequestCancel(BridgeRequestCancel {
            request_id: "req".to_string(),
            reason: "downstream_closed".to_string(),
            response_started: false,
        }));
        round_trips(BridgeMessage::McpRequestCancel(McpRequestCancel {
            request_id: "mcp-req".to_string(),
            reason: "bridge_backpressure_full".to_string(),
            response_started: true,
        }));
        round_trips(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
            request_id: "rt-req".to_string(),
            code: Some(1001),
            reason: Some("client closed".to_string()),
            response_started: true,
        }));
    }

    #[test]
    fn config_snapshot_round_trips() {
        round_trips(BridgeMessage::ConfigSnapshot(ConfigSnapshot {
            version: 7,
            keys: vec![crate::protocol::ClientRoute {
                key_hash: "hash".to_string(),
                key_prefix: "pref".to_string(),
                user_id: 42,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: crate::protocol::RelayIpPolicy {
                allowed_cidrs: vec!["10.0.0.0/8".to_string()],
                trusted_proxy_cidrs: vec!["192.168.0.0/16".to_string()],
            },
        }));
    }

    #[test]
    fn missing_trailing_field_fails_bincode_decode() {
        let message = BridgeMessage::RequestCancel(BridgeRequestCancel {
            request_id: "req".to_string(),
            reason: "downstream_closed".to_string(),
            response_started: false,
        });
        let mut encoded = encode_payload(&message).unwrap();
        encoded.truncate(encoded.len() - 1);
        let err = decode_payload(&encoded).unwrap_err();
        assert!(
            err.chain()
                .any(|cause| cause.to_string().contains("unexpected end of file")),
            "{err:?}"
        );
    }
}
