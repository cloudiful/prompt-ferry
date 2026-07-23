use serde::{Deserialize, Serialize};

use crate::cli::{RelayArgs, ServeArgs};

use super::{BridgeEncryptionMode, TlsMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServeConfig {
    pub internal_worker_bind: String,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            internal_worker_bind: "127.0.0.1:8788".to_string(),
        }
    }
}

impl ServeConfig {
    pub fn merge_args(mut self, args: ServeArgs) -> Self {
        if let Some(bind) = args.internal_worker_bind {
            self.internal_worker_bind = bind;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    pub bind: String,
    pub worker_bind: String,
    pub client_token: String,
    pub worker_token: String,
    pub request_timeout_seconds: u64,
    pub worker_heartbeat_timeout_seconds: u64,
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
            worker_heartbeat_timeout_seconds: 90,
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

impl RelayConfig {
    pub fn merge_args(mut self, args: RelayArgs) -> Self {
        if let Some(bind) = args.bind {
            self.bind = bind;
        }
        if let Some(bind) = args.worker_bind {
            self.worker_bind = bind;
        }
        if let Some(token) = args.client_token {
            self.client_token = token;
        }
        if let Some(token) = args.worker_token {
            self.worker_token = token;
        }
        if let Some(timeout) = args.request_timeout_seconds {
            self.request_timeout_seconds = timeout;
        }
        if let Some(timeout) = args.worker_heartbeat_timeout_seconds {
            self.worker_heartbeat_timeout_seconds = timeout;
        }
        if let Some(mode) = args.tls_mode {
            self.tls_mode = mode;
        }
        if let Some(path) = args.tls_cert {
            self.tls_cert = path;
        }
        if let Some(path) = args.tls_key {
            self.tls_key = path;
        }
        if let Some(path) = args.tls_client_ca {
            self.tls_client_ca = path;
        }
        if let Some(mode) = args.worker_tls_mode {
            self.worker_tls_mode = mode;
        }
        if let Some(path) = args.worker_tls_cert {
            self.worker_tls_cert = path;
        }
        if let Some(path) = args.worker_tls_key {
            self.worker_tls_key = path;
        }
        if let Some(path) = args.worker_tls_client_ca {
            self.worker_tls_client_ca = path;
        }
        if let Some(mode) = args.bridge_encryption_mode {
            self.bridge_encryption_mode = mode;
        }
        if let Some(key) = args.bridge_encryption_key {
            self.bridge_encryption_key = key;
        }
        self
    }
}
