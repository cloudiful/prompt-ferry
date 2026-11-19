use std::collections::{HashMap, HashSet};

use super::transport::PeerWriter;
use crate::protocol::BridgeMessage;
use crate::worker::runtime::{BridgeSender, bridge::BridgeSendError};
use anyhow::{Result, bail};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) fn owned_sender(sender: BridgeSender, ownership: Arc<Mutex<Ownership>>) -> BridgeSender {
    sender.with_wire_size(Arc::new(move |message| {
        let ownership = ownership.clone();
        Box::pin(async move {
            let mut mapped = message.clone();
            if ownership.lock().await.outbound(&mut mapped).is_none() {
                return Ok(0);
            }
            crate::bridge_wire::encode_message(&mapped)
                .map(|bytes| bytes.len())
                .map_err(|error| BridgeSendError::Encoding(error.to_string()))
        })
    }))
}

pub(super) struct OwnedPeerWriter<W> {
    pub(super) writer: W,
    pub(super) ownership: Arc<Mutex<Ownership>>,
}

impl<W: PeerWriter + Send> PeerWriter for OwnedPeerWriter<W> {
    async fn send_binary(&mut self, bytes: Vec<u8>) -> Result<()> {
        let mut message = crate::bridge_wire::decode_message(&bytes)?;
        let mut ownership = self.ownership.lock().await;
        let Some(terminal) = ownership.outbound(&mut message) else {
            return Ok(());
        };
        let bytes = crate::bridge_wire::encode_message(&message)?;
        self.writer.send_binary(bytes).await?;
        if let Some(internal) = terminal {
            ownership.finish(&internal);
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        self.writer.flush().await
    }
}

const MAX_ACTIVE: usize = 1024;
const MAX_ID_BYTES: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Http,
    Mcp,
    Realtime,
}

struct Entry {
    internal: String,
    kind: Kind,
}

#[derive(Default)]
pub(super) struct Ownership {
    originals: HashMap<String, Entry>,
    internals: HashMap<String, String>,
}

impl Ownership {
    pub(super) fn inbound(&mut self, message: &mut BridgeMessage) -> Result<bool> {
        use BridgeMessage::*;
        let (id, kind, start) = match message {
            RequestStart(v) => (&mut v.request_id, Kind::Http, true),
            RequestChunk(v) => (&mut v.request_id, Kind::Http, false),
            RequestEnd(v) => (&mut v.request_id, Kind::Http, false),
            RequestCancel(v) => (&mut v.request_id, Kind::Http, false),
            McpRequestStart(v) => (&mut v.request_id, Kind::Mcp, true),
            McpRequestChunk(v) => (&mut v.request_id, Kind::Mcp, false),
            McpRequestEnd(v) => (&mut v.request_id, Kind::Mcp, false),
            McpRequestCancel(v) => (&mut v.request_id, Kind::Mcp, false),
            RealtimeSessionStart(v) => (&mut v.request_id, Kind::Realtime, true),
            RealtimeClientEvent(v) => (&mut v.request_id, Kind::Realtime, false),
            RealtimeSessionClose(v) => (&mut v.request_id, Kind::Realtime, false),
            Ping | Pong => return Ok(true),
            RealtimeServerEvent(_)
            | McpResponseStart(_)
            | McpResponseChunk(_)
            | McpResponseEnd(_)
            | ConfigSnapshot(_)
            | ResponseStart(_)
            | ResponseChunk(_)
            | ResponseEnd(_)
            | ResponseError(_) => return Ok(false),
        };
        if start {
            if id.is_empty() || id.len() > MAX_ID_BYTES {
                bail!("invalid peer request id");
            }
            if self.originals.contains_key(id) {
                bail!("duplicate live peer request id");
            }
            if self.originals.len() >= MAX_ACTIVE {
                bail!("peer request limit exceeded");
            }
            let original = id.clone();
            let internal = uuid::Uuid::new_v4().to_string();
            self.originals.insert(
                original.clone(),
                Entry {
                    internal: internal.clone(),
                    kind,
                },
            );
            self.internals.insert(internal.clone(), original);
            *id = internal;
            Ok(true)
        } else if let Some(entry) = self.originals.get(id).filter(|entry| entry.kind == kind) {
            *id = entry.internal.clone();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) fn outbound(&self, message: &mut BridgeMessage) -> Option<Option<String>> {
        use BridgeMessage::*;
        let (id, terminal, expected) = match message {
            ResponseStart(v) => (&mut v.request_id, false, None),
            ResponseChunk(v) => (&mut v.request_id, false, Some(Kind::Http)),
            ResponseEnd(v) => (&mut v.request_id, true, Some(Kind::Http)),
            ResponseError(v) => (&mut v.request_id, true, None),
            McpResponseStart(v) => (&mut v.request_id, false, Some(Kind::Mcp)),
            McpResponseChunk(v) => (&mut v.request_id, false, Some(Kind::Mcp)),
            McpResponseEnd(v) => (&mut v.request_id, true, Some(Kind::Mcp)),
            RealtimeServerEvent(v) => (&mut v.request_id, false, Some(Kind::Realtime)),
            RealtimeSessionClose(v) => (&mut v.request_id, true, Some(Kind::Realtime)),
            ConfigSnapshot(_) | Ping | Pong => return Some(None),
            RequestStart(_)
            | RequestChunk(_)
            | RequestEnd(_)
            | RequestCancel(_)
            | McpRequestStart(_)
            | McpRequestChunk(_)
            | McpRequestEnd(_)
            | McpRequestCancel(_)
            | RealtimeSessionStart(_)
            | RealtimeClientEvent(_) => return None,
        };
        let internal = id.clone();
        let original = self.internals.get(&internal)?;
        let entry = self.originals.get(original)?;
        if expected.is_some_and(|kind| kind != entry.kind) {
            return None;
        }
        *id = original.clone();
        Some(terminal.then_some(internal))
    }

    pub(super) fn finish(&mut self, internal: &str) {
        if let Some(original) = self.internals.remove(internal) {
            self.originals.remove(&original);
        }
    }

    pub(super) fn active(&self) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        let mut http = HashSet::new();
        let mut mcp = HashSet::new();
        let mut realtime = HashSet::new();
        for entry in self.originals.values() {
            match entry.kind {
                Kind::Http => &mut http,
                Kind::Mcp => &mut mcp,
                Kind::Realtime => &mut realtime,
            }
            .insert(entry.internal.clone());
        }
        (http, mcp, realtime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::runtime::node::peer::transport::{MemoryPeer, PeerFrame, PeerReader};

    fn http(id: &str) -> BridgeMessage {
        serde_json::from_value(serde_json::json!({"type":"request_start", "request_id":id, "method":"POST", "path":"/v1/chat"})).unwrap()
    }
    fn mcp(id: &str) -> BridgeMessage {
        serde_json::from_value(serde_json::json!({"type":"mcp_request_start", "request_id":id, "server_name":null, "method":"POST", "path":"/mcp", "headers":[]})).unwrap()
    }
    fn internal(message: &BridgeMessage) -> String {
        match message {
            BridgeMessage::RequestStart(v) => v.request_id.clone(),
            BridgeMessage::McpRequestStart(v) => v.request_id.clone(),
            _ => panic!("not a start"),
        }
    }

    #[test]
    fn collision_is_scoped_and_bounds_enforced() {
        let mut a = Ownership::default();
        let mut b = Ownership::default();
        let mut one = http("same");
        let mut two = http("same");
        assert!(a.inbound(&mut one).unwrap());
        assert!(b.inbound(&mut two).unwrap());
        let ai = internal(&one);
        let bi = internal(&two);
        assert_ne!(ai, bi);
        let mut cancel: BridgeMessage = serde_json::from_value(
            serde_json::json!({"type":"request_cancel", "request_id":"same", "reason":"cancelled"}),
        )
        .unwrap();
        assert!(a.inbound(&mut cancel).unwrap());
        assert!(matches!(cancel, BridgeMessage::RequestCancel(ref v) if v.request_id == ai));
        a.finish(&ai);
        assert_eq!(b.active().0, HashSet::from([bi]));
        assert!(
            !a.inbound(
                &mut serde_json::from_value(
                    serde_json::json!({"type":"request_end", "request_id":"same"})
                )
                .unwrap()
            )
            .unwrap()
        );
        assert!(a.inbound(&mut http("")).is_err());
        assert!(a.inbound(&mut http(&"x".repeat(MAX_ID_BYTES + 1))).is_err());
        for id in 0..MAX_ACTIVE {
            assert!(a.inbound(&mut http(&id.to_string())).unwrap());
        }
        assert!(a.inbound(&mut http("0")).is_err());
        assert!(a.inbound(&mut http("overflow")).is_err());
    }

    #[test]
    fn realtime_and_error_terminals_are_correlated_and_scoped() {
        let mut ownership = Ownership::default();
        let mut start: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"realtime_session_start", "request_id":"real", "model":"m", "path":"/v1/realtime"
        }))
        .unwrap();
        assert!(ownership.inbound(&mut start).unwrap());
        let BridgeMessage::RealtimeSessionStart(start) = start else {
            panic!("start")
        };
        let mut event: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"realtime_client_event", "request_id":"real", "event_json":"{}"
        }))
        .unwrap();
        assert!(ownership.inbound(&mut event).unwrap());
        assert!(
            matches!(event, BridgeMessage::RealtimeClientEvent(ref v) if v.request_id == start.request_id)
        );
        let mut close: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"realtime_session_close", "request_id":start.request_id
        }))
        .unwrap();
        let terminal = ownership.outbound(&mut close).unwrap().unwrap();
        assert!(
            matches!(close, BridgeMessage::RealtimeSessionClose(ref v) if v.request_id == "real")
        );
        ownership.finish(&terminal);
        assert!(ownership.active().2.is_empty());
        let mut request = mcp("mcp");
        ownership.inbound(&mut request).unwrap();
        let mut error: BridgeMessage = serde_json::from_value(serde_json::json!({
            "type":"response_error", "request_id":internal(&request), "status":500, "code":"failure", "message":"failed"
        })).unwrap();
        let terminal = ownership.outbound(&mut error).unwrap().unwrap();
        assert!(matches!(error, BridgeMessage::ResponseError(ref v) if v.request_id == "mcp"));
        ownership.finish(&terminal);
        assert!(ownership.active().1.is_empty());
    }

    #[tokio::test]
    async fn failed_terminal_send_keeps_owner_for_disconnect_cleanup() {
        struct Failed;
        impl PeerWriter for Failed {
            async fn send_binary(&mut self, _: Vec<u8>) -> Result<()> {
                bail!("sink failure")
            }
            async fn flush(&mut self) -> Result<()> {
                Ok(())
            }
        }
        let ownership = Arc::new(Mutex::new(Ownership::default()));
        let mut request = http("wire");
        ownership.lock().await.inbound(&mut request).unwrap();
        let id = internal(&request);
        let mut writer = OwnedPeerWriter {
            writer: Failed,
            ownership: ownership.clone(),
        };
        let message = BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd {
            request_id: id.clone(),
        });
        assert!(
            writer
                .send_binary(crate::bridge_wire::encode_message(&message).unwrap())
                .await
                .is_err()
        );
        assert_eq!(ownership.lock().await.active().0, HashSet::from([id]));
    }

    #[tokio::test]
    async fn sent_http_and_mcp_terminals_free_entries_and_preserve_wire_ids() {
        let ownership = Arc::new(Mutex::new(Ownership::default()));
        let (peer, local) = MemoryPeer::pair(8);
        let (mut peer_reader, _peer_writer) = peer.into_halves();
        let (_, writer) = local.into_halves();
        let mut writer = OwnedPeerWriter {
            writer,
            ownership: ownership.clone(),
        };
        for n in 0..(MAX_ACTIVE + 20) {
            let wire_id = format!("wire-{n}");
            let mut request = if n % 2 == 0 {
                http(&wire_id)
            } else {
                mcp(&wire_id)
            };
            assert!(ownership.lock().await.inbound(&mut request).unwrap());
            let id = internal(&request);
            let message = if n % 2 == 0 {
                BridgeMessage::ResponseEnd(crate::protocol::ResponseEnd { request_id: id })
            } else {
                BridgeMessage::McpResponseEnd(crate::protocol::McpResponseEnd { request_id: id })
            };
            writer
                .send_binary(crate::bridge_wire::encode_message(&message).unwrap())
                .await
                .unwrap();
            let Some(PeerFrame::Binary(bytes)) = peer_reader.recv().await.unwrap() else {
                panic!("missing response")
            };
            match crate::bridge_wire::decode_message(&bytes).unwrap() {
                BridgeMessage::ResponseEnd(v) => assert_eq!(v.request_id, wire_id),
                BridgeMessage::McpResponseEnd(v) => assert_eq!(v.request_id, wire_id),
                _ => panic!("wrong response"),
            }
            let active = ownership.lock().await.active();
            assert!(active.0.is_empty() && active.1.is_empty() && active.2.is_empty());
        }
    }
}
