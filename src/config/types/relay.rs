use serde::{Deserialize, Serialize};

use super::{BridgeEncryptionMode, TlsMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    pub bind: String,
    pub worker_bind: String,
    pub client_token: String,
    pub worker_token: String,
    pub request_timeout_seconds: u64,
    /// Grace given to the public listener after shutdown is signalled.
    ///
    /// It must exceed the worker's own drain so an already accepted response
    /// can still travel through the bridge before the relay drops it. Defaults
    /// to 130 seconds: the 120-second worker default plus a flush margin.
    pub shutdown_drain_seconds: u64,
    pub worker_heartbeat_timeout_seconds: u64,
    pub response_stream_buffer: usize,
    pub response_stream_max_bytes: usize,
    pub response_stream_backpressure_timeout_ms: u64,
    pub tls_mode: TlsMode,
    pub tls_cert: String,
    pub tls_key: String,
    pub tls_client_ca: String,
    pub worker_tls_mode: TlsMode,
    pub worker_tls_cert: String,
    pub worker_tls_key: String,
    pub worker_tls_client_ca: String,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub bridge_encryption_key: String,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".to_string(),
            worker_bind: "127.0.0.1:8788".to_string(),
            client_token: "change-me-client-token".to_string(),
            worker_token: "change-me-worker-token".to_string(),
            request_timeout_seconds: 300,
            shutdown_drain_seconds: 130,
            worker_heartbeat_timeout_seconds: 90,
            response_stream_buffer: 256,
            response_stream_max_bytes: 16 * 1024 * 1024,
            response_stream_backpressure_timeout_ms: 5_000,
            tls_mode: TlsMode::Off,
            tls_cert: String::new(),
            tls_key: String::new(),
            tls_client_ca: String::new(),
            worker_tls_mode: TlsMode::Off,
            worker_tls_cert: String::new(),
            worker_tls_key: String::new(),
            worker_tls_client_ca: String::new(),
            bridge_encryption_mode: BridgeEncryptionMode::Off,
            bridge_encryption_key: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RelayConfig;

    #[test]
    fn defaults_keep_response_buffer_bounded() {
        let config = RelayConfig::default();

        assert_eq!(config.response_stream_buffer, 256);
        assert_eq!(config.response_stream_max_bytes, 16 * 1024 * 1024);
        assert_eq!(config.response_stream_backpressure_timeout_ms, 5_000);
    }

    #[test]
    fn shutdown_drain_default_outlives_the_worker_drain() {
        assert_eq!(RelayConfig::default().shutdown_drain_seconds, 130);
    }
}
