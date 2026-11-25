//! The peer-local wire envelope of a node link.
//!
//! The envelope is the only thing that the node links add to the existing
//! bridge codec: the payload of an envelope is a bridge wire frame, so the
//! business protocol of the nodes stays exactly what it was, while the route
//! control plane and the path of a request start travel next to it.
//!
//! The envelope is bounded on purpose. Its header never grows beyond
//! [`PEER_ENVELOPE_MAX_HEADER_BYTES`], the path of an envelope never names
//! more than [`MAX_PEER_ROUTE_NODES`] nodes, and every length is checked
//! before a buffer is reserved.

use std::fmt;

use anyhow::{Context, anyhow, bail};
use uuid::Uuid;

use crate::bridge_wire;
use crate::protocol::{BridgeMessage, ConfigSnapshot};
use crate::worker::runtime::node::identity::NodeId;

/// The peer protocol version of a node link.
///
/// The peer protocol is independent of the protocol of the database gate,
/// which keeps its own version.
pub const PEER_PROTOCOL_VERSION: u32 = 2;

/// The version of the peer envelope.
pub const PEER_ENVELOPE_VERSION: u8 = 2;

/// The most nodes of a route: three distinct nodes and two peer links.
pub const MAX_PEER_ROUTE_NODES: usize = 3;

/// The bytes of one node id in the header of an envelope.
pub const PEER_ENVELOPE_NODE_BYTES: usize = 16;

/// The fixed part of the header of an envelope.
///
/// The header holds the version, the kind, the number of nodes of the path,
/// the generation of the advertisement and the length of the payload.
pub const PEER_ENVELOPE_FIXED_HEADER_BYTES: usize = 16;

/// The biggest header of an envelope.
pub const PEER_ENVELOPE_MAX_HEADER_BYTES: usize =
    PEER_ENVELOPE_FIXED_HEADER_BYTES + MAX_PEER_ROUTE_NODES * PEER_ENVELOPE_NODE_BYTES;

/// The kind of an envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerEnvelopeKind {
    /// A route advertisement that carries the selected snapshot of the
    /// database node.
    RouteUpdate,
    /// The withdrawal of the route of a link.
    RouteWithdraw,
    /// The start of a request with its full origin to database path.
    RequestStart,
    /// Any other bridge message of a link.
    Bridge,
}

impl PeerEnvelopeKind {
    /// The wire code of the kind.
    fn code(self) -> u8 {
        match self {
            Self::RouteUpdate => 1,
            Self::RouteWithdraw => 2,
            Self::RequestStart => 3,
            Self::Bridge => 4,
        }
    }

    /// The kind of a wire code.
    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::RouteUpdate),
            2 => Some(Self::RouteWithdraw),
            3 => Some(Self::RequestStart),
            4 => Some(Self::Bridge),
            _ => None,
        }
    }
}

/// Why a path is not a valid path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerPathError {
    /// The path names more nodes than a bounded route may hold.
    TooLong,
    /// The path names the same node twice.
    RepeatedNode,
}

impl fmt::Display for PeerPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(f, "a path holds at most {MAX_PEER_ROUTE_NODES} nodes"),
            Self::RepeatedNode => f.write_str("a path names the same node twice"),
        }
    }
}

impl std::error::Error for PeerPathError {}

/// The ordered node ids of a route.
///
/// A path is only the shape of a route: whether the nodes of the path are
/// adjacent to each other and whether the path ends at the database node of
/// this node is decided by the route state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerPath {
    nodes: Vec<NodeId>,
}

impl PeerPath {
    /// Build a path from its nodes.
    ///
    /// The path is bounded and its nodes are distinct; the adjacency of the
    /// nodes is not checked here.
    pub fn new(nodes: Vec<NodeId>) -> Result<Self, PeerPathError> {
        if nodes.len() > MAX_PEER_ROUTE_NODES {
            return Err(PeerPathError::TooLong);
        }
        for (index, node) in nodes.iter().enumerate() {
            if nodes[..index].contains(node) {
                return Err(PeerPathError::RepeatedNode);
            }
        }
        Ok(Self { nodes })
    }

    /// The nodes of the path.
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    /// How many nodes the path holds.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the path holds no node at all.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether the path names the node.
    pub fn contains(&self, node: NodeId) -> bool {
        self.nodes.contains(&node)
    }

    /// The position of the node in the path.
    pub fn position(&self, node: NodeId) -> Option<usize> {
        self.nodes.iter().position(|candidate| *candidate == node)
    }

    /// The path from the node at the position to its end.
    pub fn suffix(&self, position: usize) -> Option<&[NodeId]> {
        self.nodes.get(position..)
    }
}

/// One envelope of a node link.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerEnvelope {
    /// The kind of the envelope.
    pub kind: PeerEnvelopeKind,
    /// The generation of the advertisement of the sending link.
    ///
    /// The counter grows with every advertisement of a link and may start
    /// over when the link is replaced, so a receiver always compares it
    /// within the generation of the link it arrived on.
    pub generation: u64,
    /// The path of the envelope, if it carries one.
    pub path: Option<PeerPath>,
    /// The bridge message of the envelope, if it carries one.
    pub message: Option<BridgeMessage>,
}

impl PeerEnvelope {
    /// A route advertisement with the snapshot of the database node.
    pub fn route_update(path: PeerPath, generation: u64, snapshot: &ConfigSnapshot) -> Self {
        Self {
            kind: PeerEnvelopeKind::RouteUpdate,
            generation,
            path: Some(path),
            message: Some(BridgeMessage::ConfigSnapshot(snapshot.clone())),
        }
    }

    /// The withdrawal of the route of a link.
    pub fn route_withdraw(generation: u64) -> Self {
        Self {
            kind: PeerEnvelopeKind::RouteWithdraw,
            generation,
            path: None,
            message: None,
        }
    }

    /// The start of a request with its full origin to database path.
    pub fn request_start(path: PeerPath, message: BridgeMessage) -> Self {
        Self {
            kind: PeerEnvelopeKind::RequestStart,
            generation: 0,
            path: Some(path),
            message: Some(message),
        }
    }

    /// Any other bridge message of a link.
    pub fn bridge(message: BridgeMessage) -> Self {
        Self {
            kind: PeerEnvelopeKind::Bridge,
            generation: 0,
            path: None,
            message: Some(message),
        }
    }

    /// The snapshot of a route advertisement.
    pub fn snapshot(&self) -> Option<&ConfigSnapshot> {
        match &self.message {
            Some(BridgeMessage::ConfigSnapshot(snapshot)) => Some(snapshot),
            _ => None,
        }
    }

    /// The wire bytes of the envelope.
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        self.validate()?;
        let payload = match &self.message {
            Some(message) => bridge_wire::encode_message(message)?,
            None => Vec::new(),
        };
        let payload_len = u32::try_from(payload.len())
            .map_err(|_| anyhow!("the peer envelope payload is too large"))?;
        let path_nodes = self.path.as_ref().map(PeerPath::nodes).unwrap_or_default();
        let header_len =
            PEER_ENVELOPE_FIXED_HEADER_BYTES + path_nodes.len() * PEER_ENVELOPE_NODE_BYTES;
        if header_len > PEER_ENVELOPE_MAX_HEADER_BYTES {
            bail!("the peer envelope header is too large: {header_len}");
        }
        let mut wire = Vec::with_capacity(header_len + payload.len());
        wire.push(PEER_ENVELOPE_VERSION);
        wire.push(self.kind.code());
        wire.push(path_nodes.len() as u8);
        wire.push(0);
        wire.extend_from_slice(&self.generation.to_le_bytes());
        wire.extend_from_slice(&payload_len.to_le_bytes());
        for node in path_nodes {
            wire.extend_from_slice(node.as_uuid().as_bytes());
        }
        wire.extend_from_slice(&payload);
        Ok(wire)
    }

    /// Read an envelope from its wire bytes.
    ///
    /// The read is strict: an unsupported version or kind, a truncated or
    /// oversized frame, trailing bytes and a path that names the same node
    /// twice are all refused.
    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() < PEER_ENVELOPE_FIXED_HEADER_BYTES {
            bail!("the peer envelope is too short ({} bytes)", bytes.len());
        }
        let version = bytes[0];
        if version != PEER_ENVELOPE_VERSION {
            bail!("unsupported peer envelope version {version}, expected {PEER_ENVELOPE_VERSION}");
        }
        let kind = PeerEnvelopeKind::from_code(bytes[1])
            .ok_or_else(|| anyhow!("unsupported peer envelope kind {}", bytes[1]))?;
        let path_count = bytes[2] as usize;
        if path_count > MAX_PEER_ROUTE_NODES {
            bail!(
                "the peer envelope path names {path_count} nodes, at most {MAX_PEER_ROUTE_NODES} are allowed"
            );
        }
        if bytes[3] != 0 {
            bail!("the reserved byte of the peer envelope must be zero");
        }
        let generation = u64::from_le_bytes(bytes[4..12].try_into().unwrap_or_default());
        let payload_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or_default()) as usize;
        if payload_len > bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES {
            bail!(
                "the peer envelope payload is too large: {payload_len} > {}",
                bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES
            );
        }
        let header_len = PEER_ENVELOPE_FIXED_HEADER_BYTES + path_count * PEER_ENVELOPE_NODE_BYTES;
        if bytes.len() < header_len {
            bail!("the peer envelope header is truncated");
        }
        if bytes.len() != header_len + payload_len {
            bail!(
                "the peer envelope length does not match its header: {} != {}",
                bytes.len(),
                header_len + payload_len
            );
        }
        let mut nodes = Vec::with_capacity(path_count);
        for index in 0..path_count {
            let start = PEER_ENVELOPE_FIXED_HEADER_BYTES + index * PEER_ENVELOPE_NODE_BYTES;
            let end = start + PEER_ENVELOPE_NODE_BYTES;
            let uuid = Uuid::from_slice(&bytes[start..end])
                .context("the peer envelope path holds an invalid node id")?;
            nodes.push(NodeId::from_uuid(uuid));
        }
        let path = if path_count == 0 {
            None
        } else {
            Some(
                PeerPath::new(nodes)
                    .map_err(|err| anyhow!("the peer envelope path is invalid: {err}"))?,
            )
        };
        let payload = &bytes[header_len..];
        let message = if payload.is_empty() {
            None
        } else {
            Some(bridge_wire::decode_message(payload).with_context(|| {
                format!(
                    "failed to decode the bridge payload of the peer envelope (frame {} bytes)",
                    payload.len()
                )
            })?)
        };
        let envelope = Self {
            kind,
            generation,
            path,
            message,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Check that the envelope carries what its kind demands.
    fn validate(&self) -> anyhow::Result<()> {
        match self.kind {
            PeerEnvelopeKind::RouteUpdate => {
                if self.path.as_ref().is_none_or(PeerPath::is_empty) {
                    bail!("a route advertisement must carry a path");
                }
                if self.snapshot().is_none() {
                    bail!("a route advertisement must carry a snapshot");
                }
            }
            PeerEnvelopeKind::RouteWithdraw => {
                if self.path.is_some() {
                    bail!("a route withdrawal must not carry a path");
                }
                if self.message.is_some() {
                    bail!("a route withdrawal must not carry a message");
                }
            }
            PeerEnvelopeKind::RequestStart => {
                if self.path.as_ref().is_none_or(PeerPath::is_empty) {
                    bail!("a request start must carry a path");
                }
                let message = self
                    .message
                    .as_ref()
                    .ok_or_else(|| anyhow!("a request start must carry a message"))?;
                if !is_start(message) {
                    bail!("a request start must carry a start message");
                }
            }
            PeerEnvelopeKind::Bridge => {
                if self.path.is_some() {
                    bail!("a bridge message must not carry a path");
                }
                if self.generation != 0 {
                    bail!("a bridge message must not carry a generation");
                }
                let message = self
                    .message
                    .as_ref()
                    .ok_or_else(|| anyhow!("a bridge message must carry a message"))?;
                if is_start(message) {
                    bail!("a start message must carry its path");
                }
            }
        }
        Ok(())
    }
}

/// Whether the bridge message starts a request.
fn is_start(message: &BridgeMessage) -> bool {
    matches!(
        message,
        BridgeMessage::RequestStart(_)
            | BridgeMessage::McpRequestStart(_)
            | BridgeMessage::RealtimeSessionStart(_)
    )
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        MAX_PEER_ROUTE_NODES, PEER_ENVELOPE_FIXED_HEADER_BYTES, PEER_ENVELOPE_MAX_HEADER_BYTES,
        PeerEnvelope, PeerEnvelopeKind, PeerPath, PeerPathError,
    };
    use crate::protocol::{
        BridgeMessage, BridgeRequestStart, ClientRoute, ConfigSnapshot, McpRequestStart,
        RealtimeSessionStart, ResponseChunk, ResponseEnd,
    };
    use crate::worker::runtime::node::identity::NodeId;

    fn id(value: u128) -> NodeId {
        NodeId::from_uuid(Uuid::from_u128(value))
    }

    fn path(nodes: Vec<NodeId>) -> PeerPath {
        PeerPath::new(nodes).expect("a bounded path")
    }

    fn http_start(request_id: &str) -> BridgeMessage {
        BridgeMessage::RequestStart(BridgeRequestStart {
            request_id: request_id.to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            request_deadline_unix_ms: 42,
            user_id: Some(7),
            route_id: Some("route".to_string()),
            client_key_hash: Some("hash".to_string()),
            request_user_agent: Some("codex/1.0".to_string()),
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
        })
    }

    fn mcp_start() -> BridgeMessage {
        BridgeMessage::McpRequestStart(McpRequestStart {
            request_id: "mcp-1".to_string(),
            server_name: Some("catalog".to_string()),
            method: "POST".to_string(),
            path: "/mcp".to_string(),
            headers: vec![],
            user_id: Some(3),
            client_key_hash: Some("hash".to_string()),
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
        })
    }

    fn realtime_start() -> BridgeMessage {
        BridgeMessage::RealtimeSessionStart(RealtimeSessionStart {
            request_id: "rt-1".to_string(),
            model: "gpt".to_string(),
            path: "/v1/realtime".to_string(),
            user_id: Some(1),
            route_id: None,
            client_key_hash: None,
            request_user_agent: None,
        })
    }

    #[test]
    fn the_header_of_an_envelope_stays_within_its_bound() {
        assert_eq!(PEER_ENVELOPE_MAX_HEADER_BYTES, 64);
        assert_eq!(
            PEER_ENVELOPE_MAX_HEADER_BYTES,
            PEER_ENVELOPE_FIXED_HEADER_BYTES + MAX_PEER_ROUTE_NODES * 16
        );
        let envelope =
            PeerEnvelope::request_start(path(vec![id(1), id(2), id(3)]), http_start("req"));
        let wire = envelope.encode().expect("the envelope encodes");
        let payload =
            crate::bridge_wire::encode_message(&http_start("req")).expect("the payload encodes");
        let header = wire.len() - payload.len();
        assert_eq!(header, PEER_ENVELOPE_MAX_HEADER_BYTES);
        assert!(header <= PEER_ENVELOPE_MAX_HEADER_BYTES);
    }

    #[test]
    fn an_http_start_round_trips_with_its_path() {
        let envelope = PeerEnvelope::request_start(path(vec![id(1), id(2)]), http_start("req"));
        let wire = envelope.encode().expect("the envelope encodes");
        let decoded = PeerEnvelope::decode(&wire).expect("the envelope decodes");
        assert_eq!(decoded.kind, PeerEnvelopeKind::RequestStart);
        assert_eq!(decoded.path, envelope.path);
        assert_eq!(decoded.message, envelope.message);
    }

    #[test]
    fn an_mcp_start_round_trips_with_its_path() {
        let envelope = PeerEnvelope::request_start(path(vec![id(1), id(2), id(3)]), mcp_start());
        let wire = envelope.encode().expect("the envelope encodes");
        let decoded = PeerEnvelope::decode(&wire).expect("the envelope decodes");
        assert_eq!(decoded.message, Some(mcp_start()));
        assert_eq!(decoded.path.unwrap().len(), 3);
    }

    #[test]
    fn a_realtime_start_round_trips_with_its_path() {
        let envelope = PeerEnvelope::request_start(path(vec![id(2), id(3)]), realtime_start());
        let wire = envelope.encode().expect("the envelope encodes");
        let decoded = PeerEnvelope::decode(&wire).expect("the envelope decodes");
        assert_eq!(decoded.message, Some(realtime_start()));
    }

    #[test]
    fn a_response_and_a_control_message_round_trip_without_a_path() {
        let chunk = PeerEnvelope::bridge(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: "req".to_string(),
            data: vec![b'x'; 16],
        }));
        let decoded = PeerEnvelope::decode(&chunk.encode().expect("the envelope encodes"))
            .expect("the envelope decodes");
        assert_eq!(decoded.kind, PeerEnvelopeKind::Bridge);
        assert!(decoded.path.is_none());
        assert_eq!(decoded.message, chunk.message);

        let end = PeerEnvelope::bridge(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: "req".to_string(),
        }));
        let decoded = PeerEnvelope::decode(&end.encode().expect("the envelope encodes"))
            .expect("the envelope decodes");
        assert_eq!(decoded.message, end.message);

        let ping = PeerEnvelope::bridge(BridgeMessage::Ping);
        let decoded = PeerEnvelope::decode(&ping.encode().expect("the envelope encodes"))
            .expect("the envelope decodes");
        assert_eq!(decoded.message, Some(BridgeMessage::Ping));
    }

    #[test]
    fn a_route_advertisement_round_trips_with_its_snapshot() {
        let snapshot = ConfigSnapshot {
            version: 3,
            keys: vec![ClientRoute {
                key_hash: "hash".to_string(),
                key_prefix: "prefix".to_string(),
                user_id: 9,
                route_id: "route".to_string(),
            }],
            relay_ip_policy: Default::default(),
        };
        let envelope = PeerEnvelope::route_update(path(vec![id(2), id(3)]), 7, &snapshot);
        let wire = envelope.encode().expect("the envelope encodes");
        let decoded = PeerEnvelope::decode(&wire).expect("the envelope decodes");
        assert_eq!(decoded.kind, PeerEnvelopeKind::RouteUpdate);
        assert_eq!(decoded.generation, 7);
        assert_eq!(decoded.snapshot(), Some(&snapshot));
    }

    #[test]
    fn a_route_withdrawal_carries_no_path_and_no_payload() {
        let envelope = PeerEnvelope::route_withdraw(2);
        let wire = envelope.encode().expect("the envelope encodes");
        assert_eq!(wire.len(), PEER_ENVELOPE_FIXED_HEADER_BYTES);
        let decoded = PeerEnvelope::decode(&wire).expect("the envelope decodes");
        assert_eq!(decoded.kind, PeerEnvelopeKind::RouteWithdraw);
        assert!(decoded.path.is_none());
        assert!(decoded.message.is_none());
    }

    #[test]
    fn an_unsupported_version_is_refused() {
        let envelope = PeerEnvelope::bridge(BridgeMessage::Ping);
        let mut wire = envelope.encode().expect("the envelope encodes");
        wire[0] = super::PEER_ENVELOPE_VERSION + 1;
        let err = PeerEnvelope::decode(&wire).expect_err("the version is refused");
        assert!(
            err.to_string()
                .contains("unsupported peer envelope version"),
            "{err}"
        );
    }

    #[test]
    fn an_unsupported_kind_is_refused() {
        let envelope = PeerEnvelope::bridge(BridgeMessage::Ping);
        let mut wire = envelope.encode().expect("the envelope encodes");
        wire[1] = 9;
        let err = PeerEnvelope::decode(&wire).expect_err("the kind is refused");
        assert!(
            err.to_string().contains("unsupported peer envelope kind"),
            "{err}"
        );
    }

    #[test]
    fn a_truncated_envelope_is_refused() {
        let envelope = PeerEnvelope::request_start(path(vec![id(1), id(2)]), http_start("req"));
        let wire = envelope.encode().expect("the envelope encodes");
        let err = PeerEnvelope::decode(&wire[..wire.len() - 3]).expect_err("the frame is refused");
        assert!(
            err.to_string().contains("does not match its header"),
            "{err}"
        );
        let err = PeerEnvelope::decode(&wire[..8]).expect_err("the header is refused");
        assert!(err.to_string().contains("too short"), "{err}");
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let envelope = PeerEnvelope::bridge(BridgeMessage::Ping);
        let mut wire = envelope.encode().expect("the envelope encodes");
        wire.push(0);
        let err = PeerEnvelope::decode(&wire).expect_err("the trailing byte is refused");
        assert!(
            err.to_string().contains("does not match its header"),
            "{err}"
        );
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_is_reserved() {
        let envelope = PeerEnvelope::bridge(BridgeMessage::Ping);
        let mut wire = envelope.encode().expect("the envelope encodes");
        wire[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let err = PeerEnvelope::decode(&wire).expect_err("the payload is refused");
        assert!(err.to_string().contains("payload is too large"), "{err}");
    }

    #[test]
    fn a_path_that_names_a_node_twice_is_refused() {
        assert_eq!(
            PeerPath::new(vec![id(1), id(1)]),
            Err(PeerPathError::RepeatedNode)
        );
        assert_eq!(
            PeerPath::new(vec![id(1), id(2), id(3), id(4)]),
            Err(PeerPathError::TooLong)
        );
    }

    #[test]
    fn a_path_with_too_many_nodes_is_refused_on_the_wire() {
        let envelope = PeerEnvelope::request_start(path(vec![id(1), id(2)]), http_start("req"));
        let mut wire = envelope.encode().expect("the envelope encodes");
        wire[2] = 4;
        let err = PeerEnvelope::decode(&wire).expect_err("the path is refused");
        assert!(err.to_string().contains("at most 3 are allowed"), "{err}");
    }

    #[test]
    fn a_start_without_a_path_is_refused() {
        let envelope = PeerEnvelope {
            kind: PeerEnvelopeKind::RequestStart,
            generation: 0,
            path: None,
            message: Some(http_start("req")),
        };
        let err = envelope.encode().expect_err("the start is refused");
        assert!(err.to_string().contains("must carry a path"), "{err}");
    }

    #[test]
    fn a_bridge_message_must_not_carry_a_path_or_a_generation() {
        let envelope = PeerEnvelope {
            kind: PeerEnvelopeKind::Bridge,
            generation: 2,
            path: None,
            message: Some(BridgeMessage::Ping),
        };
        let err = envelope.encode().expect_err("the message is refused");
        assert!(
            err.to_string().contains("must not carry a generation"),
            "{err}"
        );
    }
}
