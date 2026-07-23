use serde::{Deserialize, Serialize};

use crate::cli::WorkerArgs;

use super::{BridgeEncryptionMode, NativeApi, WorkerTlsMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerConfig {
    pub relay_urls: Vec<String>,
    pub worker_token: String,
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub upstream_native_api: NativeApi,
    pub connect_timeout_seconds: u64,
    pub admin_bind: String,
    pub database_url: String,
    pub bootstrap_admin_login: String,
    pub bootstrap_admin_password: String,
    pub tls_mode: WorkerTlsMode,
    pub relay_ca: String,
    pub client_cert: String,
    pub client_key: String,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub bridge_encryption_key: String,
    pub relay_secret_master_key: String,
    pub valkey_url: String,
    pub valkey_ttl_seconds: u64,
    pub session_ttl_seconds: u64,
    pub endpoint_model_cache_ttl_seconds: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
            worker_token: "change-me-worker-token".to_string(),
            upstream_base_url: "https://api.openai.com".to_string(),
            upstream_api_key: String::new(),
            upstream_native_api: NativeApi::Responses,
            connect_timeout_seconds: 30,
            admin_bind: "127.0.0.1:8789".to_string(),
            database_url: String::new(),
            bootstrap_admin_login: "admin".to_string(),
            bootstrap_admin_password: String::new(),
            tls_mode: WorkerTlsMode::Auto,
            relay_ca: String::new(),
            client_cert: String::new(),
            client_key: String::new(),
            bridge_encryption_mode: BridgeEncryptionMode::Off,
            bridge_encryption_key: String::new(),
            relay_secret_master_key: String::new(),
            valkey_url: String::new(),
            valkey_ttl_seconds: 24 * 60 * 60,
            session_ttl_seconds: 7 * 24 * 60 * 60,
            endpoint_model_cache_ttl_seconds: 300,
        }
    }
}

impl WorkerConfig {
    pub fn merge_args(mut self, args: WorkerArgs) -> Self {
        if !args.relay_url.is_empty() {
            self.relay_urls = args.relay_url;
        }
        if let Some(token) = args.worker_token {
            self.worker_token = token;
        }
        if let Some(url) = args.upstream_base_url {
            self.upstream_base_url = url;
        }
        if let Some(key) = args.upstream_api_key {
            self.upstream_api_key = key;
        }
        if let Some(api) = args.upstream_native_api {
            self.upstream_native_api = api;
        }
        if let Some(timeout) = args.connect_timeout_seconds {
            self.connect_timeout_seconds = timeout;
        }
        if let Some(bind) = args.admin_bind {
            self.admin_bind = bind;
        }
        if let Some(url) = args.database_url {
            self.database_url = url;
        }
        if let Some(login) = args.bootstrap_admin_login {
            self.bootstrap_admin_login = login;
        }
        if let Some(password) = args.bootstrap_admin_password {
            self.bootstrap_admin_password = password;
        }
        if let Some(mode) = args.tls_mode {
            self.tls_mode = mode;
        }
        if let Some(path) = args.relay_ca {
            self.relay_ca = path;
        }
        if let Some(path) = args.client_cert {
            self.client_cert = path;
        }
        if let Some(path) = args.client_key {
            self.client_key = path;
        }
        if let Some(mode) = args.bridge_encryption_mode {
            self.bridge_encryption_mode = mode;
        }
        if let Some(key) = args.bridge_encryption_key {
            self.bridge_encryption_key = key;
        }
        if let Some(url) = args.valkey_url {
            self.valkey_url = url;
        }
        if let Some(ttl) = args.valkey_ttl_seconds {
            self.valkey_ttl_seconds = ttl;
        }
        if let Some(ttl) = args.session_ttl_seconds {
            self.session_ttl_seconds = ttl;
        }
        if let Some(ttl) = args.endpoint_model_cache_ttl_seconds {
            self.endpoint_model_cache_ttl_seconds = ttl.max(1);
        }
        self
    }
}

pub fn normalize_relay_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}
