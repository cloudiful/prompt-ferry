use std::fmt;
use std::net::SocketAddr;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize};

use crate::relay_secrets::RelaySecretManager;

use super::{McpWarmupMode, NativeApi, RelayConfig, TlsMode, WorkerConfig};

/// Path every node serves for peer traffic.
pub const NODE_ENDPOINT_PATH: &str = "/_internal/node";

const MASTER_SECRET_BYTES: usize = 32;
const DEFAULT_BIND_PORT: u16 = 8787;

/// TLS mode of the node's single listener.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeTlsMode {
    #[default]
    Off,
    Server,
    Mtls,
}

/// Root node configuration, read from `PROMPT_FERRY__*`.
///
/// Serialization intentionally drops [`MasterSecret`]; a serialized node
/// configuration is never a source for the secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    pub bind: SocketAddr,
    #[serde(skip_serializing)]
    pub master_secret: Option<MasterSecret>,
    /// PostgreSQL URL. An empty value means this node runs without a database.
    pub database_url: String,
    pub peer_urls: Vec<String>,
    pub tls_mode: NodeTlsMode,
    pub tls_cert: String,
    pub tls_key: String,
    pub tls_client_ca: String,
    /// Optional first-start seed for the database-owned shared Valkey setting.
    /// The database row always wins.
    pub valkey_url: String,
    pub log_level: String,
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub upstream_native_api: NativeApi,
    pub connect_timeout_seconds: u64,
    pub request_timeout_seconds: u64,
    pub worker_heartbeat_timeout_seconds: u64,
    pub shutdown_drain_seconds: u64,
    pub response_stream_buffer: usize,
    pub response_stream_max_bytes: usize,
    pub response_stream_backpressure_timeout_ms: u64,
    pub session_ttl_seconds: u64,
    pub local_session_max_entries: usize,
    pub max_upstream_response_bytes: usize,
    pub max_raw_response_capture_bytes: usize,
    pub max_response_text_capture_bytes: usize,
    pub endpoint_model_cache_ttl_seconds: u64,
    pub mcp_warmup: McpWarmupMode,
    pub mcp_allowed_origins: Vec<String>,
    /// Public HTTPS base URL used to build the personal MCP OAuth callback.
    /// Empty disables personal OAuth only.
    pub mcp_oauth_redirect_base: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], DEFAULT_BIND_PORT)),
            master_secret: None,
            database_url: String::new(),
            peer_urls: Vec::new(),
            tls_mode: NodeTlsMode::Off,
            tls_cert: String::new(),
            tls_key: String::new(),
            tls_client_ca: String::new(),
            valkey_url: String::new(),
            log_level: "info".to_string(),
            upstream_base_url: "https://api.openai.com".to_string(),
            upstream_api_key: String::new(),
            upstream_native_api: NativeApi::Responses,
            connect_timeout_seconds: 30,
            request_timeout_seconds: 300,
            worker_heartbeat_timeout_seconds: 90,
            shutdown_drain_seconds: 120,
            response_stream_buffer: 256,
            response_stream_max_bytes: 16 * 1024 * 1024,
            response_stream_backpressure_timeout_ms: 5_000,
            session_ttl_seconds: 7 * 24 * 60 * 60,
            local_session_max_entries: 10_000,
            max_upstream_response_bytes: 64 * 1024 * 1024,
            max_raw_response_capture_bytes: 4 * 1024 * 1024,
            max_response_text_capture_bytes: 1024 * 1024,
            endpoint_model_cache_ttl_seconds: 300,
            mcp_warmup: McpWarmupMode::All,
            mcp_allowed_origins: Vec::new(),
            mcp_oauth_redirect_base: String::new(),
        }
    }
}

impl NodeConfig {
    /// Validate the configuration without reading files, environment or
    /// services.
    pub fn validate(&self) -> Result<(), NodeConfigError> {
        if self.master_secret.is_none() {
            return Err(NodeConfigError::MissingMasterSecret);
        }
        if self.tls_mode != NodeTlsMode::Off {
            if self.tls_cert.trim().is_empty() {
                return Err(NodeConfigError::MissingTlsCertificate);
            }
            if self.tls_key.trim().is_empty() {
                return Err(NodeConfigError::MissingTlsKey);
            }
            if self.tls_mode == NodeTlsMode::Mtls && self.tls_client_ca.trim().is_empty() {
                return Err(NodeConfigError::MissingTlsClientCa);
            }
        }
        for peer_url in &self.peer_urls {
            normalize_peer_url(peer_url)?;
        }
        if !self.database_url.trim().is_empty() && !is_postgres_url(&self.database_url) {
            return Err(NodeConfigError::InvalidDatabaseUrl);
        }
        Ok(())
    }

    /// The internal worker configuration of this node.
    ///
    /// The node executes its own requests, so it never dials a relay and it
    /// has no worker token. The master secret is the only key material: it
    /// encrypts the configuration at rest and it never authenticates a
    /// request.
    pub fn worker_config(&self, node_id: &str) -> anyhow::Result<WorkerConfig> {
        let master = self
            .master_secret
            .as_ref()
            .ok_or(NodeConfigError::MissingMasterSecret)?;
        Ok(WorkerConfig {
            relay_urls: Vec::new(),
            worker_identity: format!("node-{node_id}"),
            worker_token: String::new(),
            upstream_base_url: self.upstream_base_url.clone(),
            upstream_api_key: self.upstream_api_key.clone(),
            upstream_native_api: self.upstream_native_api,
            connect_timeout_seconds: self.connect_timeout_seconds,
            database_url: self.database_url.trim().to_string(),
            admin_bind: self.bind.to_string(),
            bootstrap_admin_login: String::new(),
            bootstrap_admin_password: String::new(),
            worker_config_encryption_key: STANDARD.encode(master.as_bytes()),
            session_ttl_seconds: self.session_ttl_seconds,
            local_session_max_entries: self.local_session_max_entries.max(1),
            shutdown_drain_seconds: self.shutdown_drain_seconds,
            mcp_warmup: self.mcp_warmup,
            max_upstream_response_bytes: self.max_upstream_response_bytes.max(1),
            max_raw_response_capture_bytes: self.max_raw_response_capture_bytes.max(1),
            max_response_text_capture_bytes: self.max_response_text_capture_bytes.max(1),
            endpoint_model_cache_ttl_seconds: self.endpoint_model_cache_ttl_seconds.max(1),
            mcp_allowed_origins: self.mcp_allowed_origins.clone(),
            mcp_oauth_redirect_base: self.mcp_oauth_redirect_base.clone(),
            ..WorkerConfig::default()
        })
    }

    /// The public relay configuration of this node.
    ///
    /// The node has no static client token: the public AI surface accepts
    /// managed client keys only, so the legacy token path stays closed for
    /// it. The relay management surface of the legacy relay is not part of
    /// this configuration.
    pub fn relay_config(&self) -> RelayConfig {
        RelayConfig {
            bind: self.bind.to_string(),
            client_token: String::new(),
            worker_token: String::new(),
            request_timeout_seconds: self.request_timeout_seconds,
            shutdown_drain_seconds: self.shutdown_drain_seconds.saturating_add(10),
            worker_heartbeat_timeout_seconds: self.worker_heartbeat_timeout_seconds,
            response_stream_buffer: self.response_stream_buffer,
            response_stream_max_bytes: self.response_stream_max_bytes,
            response_stream_backpressure_timeout_ms: self.response_stream_backpressure_timeout_ms,
            tls_mode: match self.tls_mode {
                NodeTlsMode::Off => TlsMode::Off,
                NodeTlsMode::Server => TlsMode::Server,
                NodeTlsMode::Mtls => TlsMode::Mtls,
            },
            tls_cert: self.tls_cert.clone(),
            tls_key: self.tls_key.clone(),
            tls_client_ca: self.tls_client_ca.clone(),
            ..RelayConfig::default()
        }
    }

    /// The secret manager of this node, built from the master secret.
    ///
    /// No key is generated and no legacy key is read: the master secret is
    /// the only key material of the node.
    pub fn secret_manager(&self) -> anyhow::Result<RelaySecretManager> {
        let master = self
            .master_secret
            .as_ref()
            .ok_or(NodeConfigError::MissingMasterSecret)?;
        Ok(RelaySecretManager::from_key_bytes(*master.as_bytes()))
    }
}

impl fmt::Debug for NodeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeConfig")
            .field("bind", &self.bind)
            .field("master_secret", &self.master_secret)
            .field("database_url", &RedactedValue(&self.database_url))
            .field(
                "peer_urls",
                &self
                    .peer_urls
                    .iter()
                    .map(|peer_url| redacted_peer_url(peer_url))
                    .collect::<Vec<_>>(),
            )
            .field("tls_mode", &self.tls_mode)
            .field("tls_cert", &self.tls_cert)
            .field("tls_key", &self.tls_key)
            .field("tls_client_ca", &self.tls_client_ca)
            .field("valkey_url", &RedactedValue(&self.valkey_url))
            .field("log_level", &self.log_level)
            .field("upstream_base_url", &RedactedValue(&self.upstream_base_url))
            .field("upstream_api_key", &RedactedValue(&self.upstream_api_key))
            .field("upstream_native_api", &self.upstream_native_api)
            .field("connect_timeout_seconds", &self.connect_timeout_seconds)
            .field("request_timeout_seconds", &self.request_timeout_seconds)
            .field(
                "worker_heartbeat_timeout_seconds",
                &self.worker_heartbeat_timeout_seconds,
            )
            .field("shutdown_drain_seconds", &self.shutdown_drain_seconds)
            .field("response_stream_buffer", &self.response_stream_buffer)
            .field("response_stream_max_bytes", &self.response_stream_max_bytes)
            .field(
                "response_stream_backpressure_timeout_ms",
                &self.response_stream_backpressure_timeout_ms,
            )
            .field("session_ttl_seconds", &self.session_ttl_seconds)
            .field("local_session_max_entries", &self.local_session_max_entries)
            .field(
                "max_upstream_response_bytes",
                &self.max_upstream_response_bytes,
            )
            .field(
                "max_raw_response_capture_bytes",
                &self.max_raw_response_capture_bytes,
            )
            .field(
                "max_response_text_capture_bytes",
                &self.max_response_text_capture_bytes,
            )
            .field(
                "endpoint_model_cache_ttl_seconds",
                &self.endpoint_model_cache_ttl_seconds,
            )
            .field("mcp_warmup", &self.mcp_warmup)
            .field("mcp_allowed_origins", &self.mcp_allowed_origins)
            .field(
                "mcp_oauth_redirect_base",
                &RedactedValue(&self.mcp_oauth_redirect_base),
            )
            .finish()
    }
}

/// The root secret of a node: the ChaCha20-Poly1305 at-rest key and, until a
/// dedicated peer credential exists, the peer authentication secret.
///
/// It wraps exactly 32 bytes decoded from standard base64. Byte access is the
/// single narrow [`MasterSecret::as_bytes`] accessor; there is no `Display`, no
/// `Serialize`, and the [`fmt::Debug`] output is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct MasterSecret([u8; MASTER_SECRET_BYTES]);

impl MasterSecret {
    pub fn as_bytes(&self) -> &[u8; MASTER_SECRET_BYTES] {
        &self.0
    }

    fn from_base64(value: &str) -> Result<Self, NodeConfigError> {
        let bytes = STANDARD
            .decode(value.trim().as_bytes())
            .map_err(|_| NodeConfigError::InvalidMasterSecret)?;
        let secret = bytes
            .try_into()
            .map_err(|_| NodeConfigError::InvalidMasterSecret)?;
        Ok(Self(secret))
    }
}

impl fmt::Debug for MasterSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterSecret(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for MasterSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        MasterSecret::from_base64(&encoded).map_err(serde::de::Error::custom)
    }
}

/// Configuration errors. Messages never echo the supplied secret, credential
/// or network location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeConfigError {
    InvalidMasterSecret,
    MissingMasterSecret,
    InvalidPeerUrl,
    InvalidDatabaseUrl,
    MissingTlsCertificate,
    MissingTlsKey,
    MissingTlsClientCa,
    IncompletePeerClientMaterial,
}

impl fmt::Display for NodeConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidMasterSecret => {
                "master secret must be standard base64 encoding exactly 32 bytes"
            }
            Self::MissingMasterSecret => "master secret is required",
            Self::InvalidPeerUrl => {
                "peer URL must be an absolute ws/wss URL with a host and no credentials, query, or fragment"
            }
            Self::InvalidDatabaseUrl => "database URL must be a PostgreSQL URL",
            Self::MissingTlsCertificate => "TLS mode requires a tls_cert path",
            Self::MissingTlsKey => "TLS mode requires a tls_key path",
            Self::MissingTlsClientCa => "mutual TLS mode requires a tls_client_ca path",
            Self::IncompletePeerClientMaterial => {
                "the peer client certificate and key must be configured together"
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for NodeConfigError {}

/// Validate a peer URL and return it in canonical form.
///
/// A peer is an absolute `ws`/`wss` URL with a host. Its path must be the node
/// endpoint; a root or empty path is normalized to [`NODE_ENDPOINT_PATH`], and
/// any other path, credentials, query or fragment is rejected. The scheme is a
/// property of the outbound target only and is independent of the local
/// listener's TLS mode.
pub fn normalize_peer_url(value: &str) -> Result<String, NodeConfigError> {
    let parsed = reqwest::Url::parse(value.trim()).map_err(|_| NodeConfigError::InvalidPeerUrl)?;
    if !matches!(parsed.scheme(), "ws" | "wss") || parsed.host_str().is_none() {
        return Err(NodeConfigError::InvalidPeerUrl);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(NodeConfigError::InvalidPeerUrl);
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(NodeConfigError::InvalidPeerUrl);
    }
    let mut canonical = parsed;
    match canonical.path() {
        "" | "/" => canonical.set_path(NODE_ENDPOINT_PATH),
        NODE_ENDPOINT_PATH => {}
        _ => return Err(NodeConfigError::InvalidPeerUrl),
    }
    Ok(canonical.to_string())
}

/// The HTTP origin of a peer URL.
///
/// A peer is reached over a websocket, but the administration pages, the API
/// and the assets of a peer are ordinary HTTP resources. The origin is
/// derived from the scheme of the peer URL, so a `wss` peer is reached over
/// HTTPS and a `ws` peer over HTTP.
pub fn peer_http_origin(value: &str) -> Result<String, NodeConfigError> {
    let canonical = normalize_peer_url(value)?;
    let parsed = reqwest::Url::parse(&canonical).map_err(|_| NodeConfigError::InvalidPeerUrl)?;
    let scheme = match parsed.scheme() {
        "ws" => "http",
        "wss" => "https",
        _ => return Err(NodeConfigError::InvalidPeerUrl),
    };
    let host = parsed.host_str().ok_or(NodeConfigError::InvalidPeerUrl)?;
    let host = if parsed.port().is_some() {
        format!("{host}:{}", parsed.port().unwrap_or_default())
    } else {
        host.to_string()
    };
    Ok(format!("{scheme}://{host}"))
}

/// Whether the peer URL of a node requires TLS on the outbound link.
///
/// The scheme of the peer URL alone decides this. It is independent of the
/// TLS mode of the local listener: a node may serve plain HTTP and still
/// reach a peer over TLS.
pub fn peer_requires_tls(value: &str) -> bool {
    value.trim().starts_with("wss://")
}

/// Validate the optional client identity of the outbound peer link.
///
/// The certificate and the key are optional as a pair: a node without them
/// authenticates the peer, and a node with both of them authenticates itself
/// as well. One of them alone is a configuration error, because it cannot
/// produce a usable identity.
pub fn validate_peer_client_material(cert: &str, key: &str) -> Result<(), NodeConfigError> {
    let has_cert = !cert.trim().is_empty();
    let has_key = !key.trim().is_empty();
    if has_cert == has_key {
        return Ok(());
    }
    Err(NodeConfigError::IncompletePeerClientMaterial)
}

fn is_postgres_url(value: &str) -> bool {
    match reqwest::Url::parse(value.trim()) {
        Ok(parsed) => {
            matches!(parsed.scheme(), "postgres" | "postgresql") && parsed.host_str().is_some()
        }
        Err(_) => false,
    }
}

struct RedactedValue<'a>(&'a str);

impl fmt::Debug for RedactedValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.trim().is_empty() {
            f.write_str("\"\"")
        } else {
            f.write_str("\"<redacted>\"")
        }
    }
}

fn redacted_peer_url(value: &str) -> String {
    match reqwest::Url::parse(value.trim()) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => "<invalid>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::{
        MasterSecret, NODE_ENDPOINT_PATH, NodeConfig, NodeConfigError, NodeTlsMode,
        normalize_peer_url, peer_http_origin, peer_requires_tls, validate_peer_client_material,
    };

    const SECRET: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";

    fn secret() -> MasterSecret {
        serde_json::from_str(&format!("\"{SECRET}\"")).expect("secret fixture parses")
    }

    fn config_json() -> String {
        format!(
            concat!(
                "{{\"bind\":\"127.0.0.1:9000\",\"master_secret\":\"{}\",",
                "\"database_url\":\"\",\"peer_urls\":[],\"tls_mode\":\"off\",",
                "\"tls_cert\":\"\",\"tls_key\":\"\",\"tls_client_ca\":\"\"}}"
            ),
            SECRET
        )
    }

    fn parse(json: &str) -> Result<NodeConfig, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn master_secret_decodes_standard_base64() {
        let decoded = secret();
        assert_eq!(decoded.as_bytes(), &[b'A'; 32]);
    }

    #[test]
    fn master_secret_rejects_invalid_base64_without_echoing_it() {
        let input = "not base64 at all!!";
        let error = serde_json::from_str::<MasterSecret>(&format!("\"{input}\""))
            .expect_err("invalid base64 fails");
        assert!(!error.to_string().contains(input));
    }

    #[test]
    fn master_secret_rejects_wrong_length() {
        let short = STANDARD.encode([1_u8; 31]);
        let long = STANDARD.encode([1_u8; 33]);
        for encoded in [short, long, String::new()] {
            let error = serde_json::from_str::<MasterSecret>(&format!("\"{encoded}\""))
                .expect_err("wrong length fails");
            assert!(
                error
                    .to_string()
                    .contains("standard base64 encoding exactly 32 bytes")
            );
        }
    }

    #[test]
    fn defaults_match_node_contract() {
        let config = NodeConfig::default();
        assert_eq!(config.bind.to_string(), "0.0.0.0:8787");
        assert_eq!(config.tls_mode, NodeTlsMode::Off);
        assert!(config.peer_urls.is_empty());
        assert_eq!(config.database_url, "");
        assert!(config.master_secret.is_none());
        assert_eq!(config.log_level, "info");
        assert_eq!(config.connect_timeout_seconds, 30);
        assert_eq!(config.shutdown_drain_seconds, 120);
        assert_eq!(config.relay_config().shutdown_drain_seconds, 130);
        assert_eq!(config.session_ttl_seconds, 7 * 24 * 60 * 60);
        assert_eq!(config.local_session_max_entries, 10_000);
        assert_eq!(config.max_upstream_response_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_raw_response_capture_bytes, 4 * 1024 * 1024);
        assert_eq!(config.max_response_text_capture_bytes, 1024 * 1024);
        assert_eq!(config.endpoint_model_cache_ttl_seconds, 300);
        assert_eq!(config.response_stream_buffer, 256);
        assert_eq!(config.response_stream_max_bytes, 16 * 1024 * 1024);
        assert_eq!(config.response_stream_backpressure_timeout_ms, 5_000);
        assert_eq!(config.upstream_base_url, "https://api.openai.com");
    }

    #[test]
    fn flat_business_settings_reach_the_internal_adapters() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            upstream_base_url: "https://upstream.example".to_string(),
            upstream_api_key: "upstream-key".to_string(),
            upstream_native_api: crate::config::NativeApi::Chat,
            connect_timeout_seconds: 9,
            request_timeout_seconds: 44,
            worker_heartbeat_timeout_seconds: 33,
            shutdown_drain_seconds: 77,
            response_stream_buffer: 7,
            response_stream_max_bytes: 8_192,
            response_stream_backpressure_timeout_ms: 250,
            session_ttl_seconds: 120,
            local_session_max_entries: 55,
            max_upstream_response_bytes: 1_000,
            max_raw_response_capture_bytes: 2_000,
            max_response_text_capture_bytes: 3_000,
            endpoint_model_cache_ttl_seconds: 17,
            mcp_warmup: crate::config::McpWarmupMode::Lazy,
            mcp_allowed_origins: vec!["https://ui.example".to_string()],
            mcp_oauth_redirect_base: "https://api.example".to_string(),
            ..NodeConfig::default()
        };
        let worker = config.worker_config("1").expect("worker adapter");
        assert_eq!(worker.upstream_base_url, "https://upstream.example");
        assert_eq!(worker.upstream_api_key, "upstream-key");
        assert_eq!(worker.upstream_native_api, crate::config::NativeApi::Chat);
        assert_eq!(worker.connect_timeout_seconds, 9);
        assert_eq!(worker.shutdown_drain_seconds, 77);
        assert_eq!(worker.session_ttl_seconds, 120);
        assert_eq!(worker.local_session_max_entries, 55);
        assert_eq!(worker.max_upstream_response_bytes, 1_000);
        assert_eq!(worker.max_raw_response_capture_bytes, 2_000);
        assert_eq!(worker.max_response_text_capture_bytes, 3_000);
        assert_eq!(worker.endpoint_model_cache_ttl_seconds, 17);
        assert_eq!(worker.mcp_warmup, crate::config::McpWarmupMode::Lazy);
        assert_eq!(worker.mcp_allowed_origins, vec!["https://ui.example"]);
        assert_eq!(worker.mcp_oauth_redirect_base, "https://api.example");

        let relay = config.relay_config();
        assert_eq!(relay.request_timeout_seconds, 44);
        assert_eq!(relay.worker_heartbeat_timeout_seconds, 33);
        assert_eq!(relay.shutdown_drain_seconds, 87);
        assert_eq!(relay.response_stream_buffer, 7);
        assert_eq!(relay.response_stream_max_bytes, 8_192);
        assert_eq!(relay.response_stream_backpressure_timeout_ms, 250);
    }

    #[test]
    fn debug_redacts_business_secrets_and_private_urls() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            valkey_url: "redis://user:valkey-password@valkey:6379".to_string(),
            upstream_base_url: "https://upstream.example/private".to_string(),
            upstream_api_key: "upstream-key".to_string(),
            mcp_oauth_redirect_base: "https://api.example/private".to_string(),
            ..NodeConfig::default()
        };
        let debug = format!("{config:?}");
        for secret in [
            SECRET,
            "valkey-password",
            "upstream-key",
            "api.example/private",
            "upstream.example/private",
        ] {
            assert!(!debug.contains(secret), "debug leaked {secret}: {debug}");
        }
        assert!(debug.contains("NodeConfig"));
    }

    #[test]
    fn deserializes_exact_field_shape() {
        let config = parse(&config_json()).expect("fixture parses");
        assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
        assert_eq!(config.master_secret.as_ref(), Some(&secret()));
        assert_eq!(config.database_url, "");
        assert!(config.peer_urls.is_empty());
        assert_eq!(config.tls_mode, NodeTlsMode::Off);
        assert_eq!(config.tls_cert, "");
        assert_eq!(config.tls_key, "");
        assert_eq!(config.tls_client_ca, "");
    }

    #[test]
    fn deserializes_tls_and_peer_fields() {
        let json = format!(
            concat!(
                "{{\"master_secret\":\"{}\",\"tls_mode\":\"mtls\",",
                "\"tls_cert\":\"/tls/cert.pem\",\"tls_key\":\"/tls/key.pem\",",
                "\"tls_client_ca\":\"/tls/ca.pem\",",
                "\"peer_urls\":[\"ws://peer-a:8787\",\"wss://peer-b{endpoint}\"]}}"
            ),
            SECRET,
            endpoint = NODE_ENDPOINT_PATH
        );
        let config = parse(&json).expect("fixture parses");
        assert_eq!(config.tls_mode, NodeTlsMode::Mtls);
        assert_eq!(config.tls_cert, "/tls/cert.pem");
        assert_eq!(config.tls_key, "/tls/key.pem");
        assert_eq!(config.tls_client_ca, "/tls/ca.pem");
        assert_eq!(
            config.peer_urls,
            vec![
                "ws://peer-a:8787".to_string(),
                format!("wss://peer-b{NODE_ENDPOINT_PATH}"),
            ]
        );
        config.validate().expect("peer target TLS is independent");
    }

    #[test]
    fn missing_master_secret_fails_validation() {
        let config = NodeConfig::default();
        assert_eq!(config.validate(), Err(NodeConfigError::MissingMasterSecret));
    }

    #[test]
    fn tls_modes_require_their_paths() {
        let mut config = NodeConfig {
            master_secret: Some(secret()),
            ..NodeConfig::default()
        };
        config.validate().expect("off needs no path");

        for mode in [NodeTlsMode::Server, NodeTlsMode::Mtls] {
            config.tls_mode = mode;
            config.tls_cert.clear();
            config.tls_key = "/tls/key.pem".to_string();
            config.tls_client_ca = "/tls/ca.pem".to_string();
            assert_eq!(
                config.validate(),
                Err(NodeConfigError::MissingTlsCertificate)
            );

            config.tls_cert = "/tls/cert.pem".to_string();
            config.tls_key = "  ".to_string();
            assert_eq!(config.validate(), Err(NodeConfigError::MissingTlsKey));
        }

        config.tls_mode = NodeTlsMode::Server;
        config.tls_key = "/tls/key.pem".to_string();
        config.tls_client_ca.clear();
        config.validate().expect("server needs no client CA");

        config.tls_mode = NodeTlsMode::Mtls;
        assert_eq!(config.validate(), Err(NodeConfigError::MissingTlsClientCa));
        config.tls_client_ca = "/tls/ca.pem".to_string();
        config.validate().expect("mtls paths complete");
    }

    #[test]
    fn tls_path_presence_never_enables_tls() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            tls_cert: "/tls/cert.pem".to_string(),
            tls_key: "/tls/key.pem".to_string(),
            ..NodeConfig::default()
        };
        assert_eq!(config.tls_mode, NodeTlsMode::Off);
        config.validate().expect("mode stays off");
    }

    #[test]
    fn normalizes_root_peer_path_to_the_node_endpoint() {
        assert_eq!(
            normalize_peer_url("ws://peer-a:8787").expect("root path"),
            format!("ws://peer-a:8787{NODE_ENDPOINT_PATH}")
        );
        assert_eq!(
            normalize_peer_url("wss://peer-b").expect("empty path"),
            format!("wss://peer-b{NODE_ENDPOINT_PATH}")
        );
        assert_eq!(
            normalize_peer_url("ws://peer-a:8787/").expect("slash path"),
            format!("ws://peer-a:8787{NODE_ENDPOINT_PATH}")
        );
        assert_eq!(
            normalize_peer_url("ws://peer-a:8787/_internal/node").expect("endpoint path"),
            format!("ws://peer-a:8787{NODE_ENDPOINT_PATH}")
        );
    }

    #[test]
    fn rejects_malformed_peer_urls() {
        for value in [
            "",
            "peer-a:8787",
            "http://peer-a:8787",
            "ftp://peer-a:8787",
            "ws://user:pass@peer-a:8787",
            "ws://user@peer-a:8787",
            "ws://",
            "ws://peer-a:8787/_internal/node?token=abc",
            "ws://peer-a:8787/_internal/node#fragment",
            "ws://peer-a:8787/other",
        ] {
            assert_eq!(
                normalize_peer_url(value),
                Err(NodeConfigError::InvalidPeerUrl),
                "expected rejection of {value:?}"
            );
        }
    }

    #[test]
    fn validates_database_url_syntax_without_echoing_credentials() {
        let mut config = NodeConfig {
            master_secret: Some(secret()),
            ..NodeConfig::default()
        };
        config.validate().expect("empty database URL means none");

        for value in [
            "postgres://user:pass@db:5432/prompt_ferry",
            "postgresql://db/prompt_ferry",
        ] {
            config.database_url = value.to_string();
            config.validate().expect("postgres URL accepted");
        }

        for value in [
            "mysql://user:pass@db:3306/prompt_ferry",
            "sqlite:///tmp/prompt-ferry.db",
            "postgres:dbname=prompt_ferry",
        ] {
            config.database_url = value.to_string();
            let error = config.validate().expect_err("non-postgres URL rejected");
            assert_eq!(error, NodeConfigError::InvalidDatabaseUrl);
            assert!(!error.to_string().contains("pass"));
        }
    }

    #[test]
    fn debug_redacts_nested_secret_database_url_and_peer_credentials() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            database_url: "postgres://user:db-password@db:5432/prompt_ferry".to_string(),
            peer_urls: vec![
                "ws://peer-a:8787".to_string(),
                "ws://user:peer-password@peer-b:8787".to_string(),
            ],
            ..NodeConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("MasterSecret(<redacted>)"));
        assert!(!debug.contains(SECRET));
        assert!(!debug.contains("db-password"));
        assert!(!debug.contains("peer-password"));
        assert!(debug.contains("ws://peer-a:8787"));
        assert!(debug.contains("ws://peer-b:8787"));

        let secret_debug = format!("{:?}", secret());
        assert_eq!(secret_debug, "MasterSecret(<redacted>)");
    }

    #[test]
    fn serialization_never_writes_the_master_secret() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            ..NodeConfig::default()
        };
        let serialized = serde_json::to_string(&config).expect("node config serializes");
        assert!(!serialized.contains(SECRET));
        assert!(!serialized.contains("master_secret"));
    }

    #[test]
    fn the_node_worker_configuration_never_dials_a_relay() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            database_url: "postgres://db/prompt_ferry".to_string(),
            ..NodeConfig::default()
        };
        let worker = config
            .worker_config("node-1")
            .expect("the worker configuration builds");
        assert!(worker.relay_urls.is_empty());
        assert!(worker.worker_token.is_empty());
        assert_eq!(worker.worker_identity, "node-node-1");
        assert_eq!(worker.database_url, "postgres://db/prompt_ferry");
        assert_eq!(
            worker.worker_config_encryption_key,
            STANDARD.encode([b'A'; 32]),
            "the master secret is the only key material of the node"
        );
    }

    #[test]
    fn the_node_relay_configuration_has_no_static_client_token() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            ..NodeConfig::default()
        };
        let relay = config.relay_config();
        assert!(relay.client_token.is_empty());
        assert!(relay.worker_token.is_empty());
        assert_eq!(relay.bind, config.bind.to_string());
    }

    #[test]
    fn the_node_secret_manager_uses_the_master_secret() {
        let config = NodeConfig {
            master_secret: Some(secret()),
            ..NodeConfig::default()
        };
        let manager = config.secret_manager().expect("the manager builds");
        let expected = crate::relay_secrets::RelaySecretManager::from_key_bytes([b'A'; 32]);
        assert!(manager.same_key(&expected));
    }

    #[test]
    fn the_peer_http_origin_follows_the_peer_scheme() {
        assert_eq!(
            peer_http_origin("ws://peer-a:8787").expect("origin"),
            "http://peer-a:8787"
        );
        assert_eq!(
            peer_http_origin("wss://peer-b").expect("origin"),
            "https://peer-b"
        );
        assert_eq!(
            peer_http_origin("wss://peer-b:9443/_internal/node").expect("origin"),
            "https://peer-b:9443"
        );
        assert!(peer_requires_tls("wss://peer-b"));
        assert!(!peer_requires_tls("ws://peer-b"));
    }

    #[test]
    fn the_peer_client_material_is_a_pair() {
        assert!(validate_peer_client_material("", "").is_ok());
        assert!(validate_peer_client_material("/tls/cert.pem", "/tls/key.pem").is_ok());
        assert_eq!(
            validate_peer_client_material("/tls/cert.pem", ""),
            Err(NodeConfigError::IncompletePeerClientMaterial)
        );
        assert_eq!(
            validate_peer_client_material("", "/tls/key.pem"),
            Err(NodeConfigError::IncompletePeerClientMaterial)
        );
    }

    #[test]
    fn a_node_without_a_master_secret_has_no_key_material() {
        let config = NodeConfig::default();
        assert!(config.secret_manager().is_err());
        assert!(config.worker_config("node-1").is_err());
    }
}
