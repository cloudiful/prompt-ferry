use serde::{Deserialize, Serialize};

use crate::cli::WorkerArgs;

use super::{BridgeEncryptionMode, NativeApi, WorkerTlsMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerMode {
    SharedManaged,
    StandaloneManaged,
}

impl WorkerMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SharedManaged => "shared-managed",
            Self::StandaloneManaged => "standalone-managed",
        }
    }

    pub(crate) fn is_shared_managed(self) -> bool {
        self == Self::SharedManaged
    }
}

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
    pub standalone_database_path: String,
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
    pub local_session_max_entries: usize,
    pub max_upstream_response_bytes: usize,
    pub max_raw_response_capture_bytes: usize,
    pub max_response_text_capture_bytes: usize,
    pub endpoint_model_cache_ttl_seconds: u64,
    pub raw_object_store_endpoint: String,
    pub raw_object_store_bucket: String,
    pub raw_object_store_region: String,
    pub raw_object_store_access_key: String,
    pub raw_object_store_secret_key: String,
    pub raw_object_store_prefix: String,
    pub raw_object_store_allow_http: bool,
    /// Browser origins allowed to call the MCP proxy. When non-empty, requests
    /// carrying an `Origin` header must match one of these entries (RFC 6454);
    /// missing `Origin` always passes. Empty keeps Origin validation disabled.
    pub mcp_allowed_origins: Vec<String>,
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
            standalone_database_path: String::new(),
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
            local_session_max_entries: 10_000,
            max_upstream_response_bytes: 64 * 1024 * 1024,
            max_raw_response_capture_bytes: 4 * 1024 * 1024,
            max_response_text_capture_bytes: 1024 * 1024,
            endpoint_model_cache_ttl_seconds: 300,
            raw_object_store_endpoint: String::new(),
            raw_object_store_bucket: String::new(),
            raw_object_store_region: "auto".to_string(),
            raw_object_store_access_key: String::new(),
            raw_object_store_secret_key: String::new(),
            raw_object_store_prefix: "prompt-ferry/raw".to_string(),
            raw_object_store_allow_http: false,
            mcp_allowed_origins: Vec::new(),
        }
    }
}

impl WorkerConfig {
    pub(crate) fn mode(&self) -> WorkerMode {
        if self.database_url.trim().is_empty() {
            WorkerMode::StandaloneManaged
        } else {
            WorkerMode::SharedManaged
        }
    }

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
        if let Some(path) = args.standalone_database_path {
            self.standalone_database_path = path;
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
        if let Some(max_entries) = args.local_session_max_entries {
            self.local_session_max_entries = max_entries.max(1);
        }
        if let Some(bytes) = args.max_upstream_response_bytes {
            self.max_upstream_response_bytes = bytes.max(1);
        }
        if let Some(bytes) = args.max_raw_response_capture_bytes {
            self.max_raw_response_capture_bytes = bytes.max(1);
        }
        if let Some(bytes) = args.max_response_text_capture_bytes {
            self.max_response_text_capture_bytes = bytes.max(1);
        }
        if let Some(ttl) = args.endpoint_model_cache_ttl_seconds {
            self.endpoint_model_cache_ttl_seconds = ttl.max(1);
        }
        if !args.mcp_allowed_origins.is_empty() {
            self.mcp_allowed_origins = args.mcp_allowed_origins;
        }
        self
    }
}

pub fn normalize_relay_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}
