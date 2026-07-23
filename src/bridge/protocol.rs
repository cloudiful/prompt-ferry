use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BridgeMessage {
    RequestStart(BridgeRequestStart),
    RequestChunk(BridgeRequestChunk),
    RequestEnd(BridgeRequestEnd),
    ApprovalPending(ApprovalPending),
    RealtimeSessionStart(RealtimeSessionStart),
    RealtimeClientEvent(RealtimeClientEventMessage),
    RealtimeServerEvent(RealtimeServerEventMessage),
    RealtimeSessionClose(RealtimeSessionClose),
    McpRequestStart(McpRequestStart),
    McpRequestChunk(McpRequestChunk),
    McpRequestEnd(McpRequestEnd),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeSessionStart {
    pub request_id: String,
    pub model: String,
    pub path: String,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub client_key_hash: Option<String>,
    #[serde(default)]
    pub request_user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeClientEventMessage {
    pub request_id: String,
    pub event_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeServerEventMessage {
    pub request_id: String,
    pub event_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeSessionClose {
    pub request_id: String,
    #[serde(default)]
    pub code: Option<u16>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeRequestStart {
    pub request_id: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub request_deadline_unix_ms: i64,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub client_key_hash: Option<String>,
    #[serde(default)]
    pub request_user_agent: Option<String>,
    #[serde(default)]
    pub http_request_content_encoding: Option<String>,
    #[serde(default)]
    pub http_request_compressed: bool,
    #[serde(default)]
    pub http_request_compressed_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeRequestChunk {
    pub request_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeRequestEnd {
    pub request_id: String,
    #[serde(default)]
    pub http_request_compressed_bytes: Option<i64>,
    #[serde(default)]
    pub http_request_decompressed_bytes: Option<i64>,
    #[serde(default)]
    pub http_request_compression_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalPending {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpRequestStart {
    pub request_id: String,
    pub server_name: Option<String>,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub http_request_content_encoding: Option<String>,
    #[serde(default)]
    pub http_request_compressed: bool,
    #[serde(default)]
    pub http_request_compressed_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpRequestChunk {
    pub request_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpRequestEnd {
    pub request_id: String,
    #[serde(default)]
    pub http_request_compressed_bytes: Option<i64>,
    #[serde(default)]
    pub http_request_decompressed_bytes: Option<i64>,
    #[serde(default)]
    pub http_request_compression_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpResponseStart {
    pub request_id: String,
    pub status: u16,
    pub content_type: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpResponseChunk {
    pub request_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpResponseEnd {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigSnapshot {
    pub version: i64,
    pub keys: Vec<ClientRoute>,
    #[serde(default)]
    pub relay_ip_policy: RelayIpPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientRoute {
    pub key_hash: String,
    pub key_prefix: String,
    pub user_id: i64,
    pub route_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RelayIpPolicy {
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub trusted_proxy_cidrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseStart {
    pub request_id: String,
    pub status: u16,
    pub content_type: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseChunk {
    pub request_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseEnd {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseError {
    pub request_id: String,
    pub status: u16,
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_request_message() {
        let message = BridgeMessage::RequestStart(BridgeRequestStart {
            request_id: "abc".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            request_deadline_unix_ms: 123,
            user_id: Some(1),
            route_id: Some("route".to_string()),
            client_key_hash: Some("hash".to_string()),
            request_user_agent: Some("codex/1.0".to_string()),
            http_request_content_encoding: Some("gzip".to_string()),
            http_request_compressed: true,
            http_request_compressed_bytes: Some(512),
        });

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: BridgeMessage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn round_trips_request_chunk_message() {
        let message = BridgeMessage::RequestChunk(BridgeRequestChunk {
            request_id: "abc".to_string(),
            data: br#"{"stream":true}"#.to_vec(),
        });

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: BridgeMessage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn round_trips_realtime_message() {
        let message = BridgeMessage::RealtimeClientEvent(RealtimeClientEventMessage {
            request_id: "rt-1".to_string(),
            event_json: r#"{"type":"session.update"}"#.to_string(),
        });

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: BridgeMessage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn config_snapshot_defaults_relay_ip_policy() {
        let snapshot: ConfigSnapshot = serde_json::from_str(r#"{"version":1,"keys":[]}"#).unwrap();
        assert_eq!(snapshot.relay_ip_policy, RelayIpPolicy::default());
    }
}
