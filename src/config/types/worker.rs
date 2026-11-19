use serde::{Deserialize, Serialize};

use crate::storage::StorageBackend;

use super::{BridgeEncryptionMode, McpWarmupMode, NativeApi, WorkerTlsMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerConfig {
    pub relay_urls: Vec<String>,
    /// Stable identity of this worker process inside one shared database.
    /// Workers register under this name so the Workers view can tell two
    /// processes apart; an empty value falls back to `<host>:<admin_bind>`.
    pub worker_identity: String,
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
    /// Worker configuration encryption key (base64-encoded 32 bytes) used to
    /// encrypt sensitive standalone worker configuration at rest, including
    /// upstream API keys, relay TLS material, client keys, and bridge keys.
    /// Current name for `WORKER_CONFIG_ENCRYPTION_KEY`; takes precedence over
    /// the legacy [`Self::relay_secret_master_key`]. When both are empty, a
    /// random key is generated and persisted under the data root as
    /// `worker-config.key`.
    pub worker_config_encryption_key: String,
    /// Legacy name for the worker configuration encryption key
    /// (`RELAY_SECRET_MASTER_KEY`). Kept for compatibility; a non-empty
    /// current [`Self::worker_config_encryption_key`] always wins.
    pub relay_secret_master_key: String,
    pub session_ttl_seconds: u64,
    pub local_session_max_entries: usize,
    /// Hard ceiling on the relay drain that runs on shutdown. The runtime
    /// stops admitting new requests immediately and keeps already accepted
    /// inference rounds alive for at most this duration before returning a
    /// synthetic shutdown response. Keep this below the relay drain and the
    /// `docker stop --time` grace period so the process exits cleanly without
    /// a SIGKILL from the orchestrator. Defaults to 120 seconds to cover a
    /// slow full response, not just worker bookkeeping.
    pub shutdown_drain_seconds: u64,
    /// MCP startup warmup strategy.
    /// * `all` (default) — eagerly warm every enabled MCP server in the
    ///   background, like the historical behaviour.
    /// * `lazy` — skip the bulk warmup; only the first cold request pays
    ///   the per-server connect cost. Useful when startup latency matters
    ///   more than first-call latency.
    /// * `off` — disable startup warmup entirely.
    pub mcp_warmup: McpWarmupMode,
    pub max_upstream_response_bytes: usize,
    pub max_raw_response_capture_bytes: usize,
    pub max_response_text_capture_bytes: usize,
    pub endpoint_model_cache_ttl_seconds: u64,
    /// Browser origins allowed to call the MCP proxy. When non-empty, requests
    /// carrying an `Origin` header must match one of these entries (RFC 6454);
    /// missing `Origin` always passes. Empty keeps Origin validation disabled.
    pub mcp_allowed_origins: Vec<String>,
    /// Public HTTPS base URL of the admin API, used to build the personal MCP
    /// OAuth callback (`<base>/api/v1/admin/mcp-servers/<id>/oauth/callback`).
    /// Empty disables personal OAuth only; nothing else changes.
    pub mcp_oauth_redirect_base: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
            worker_identity: String::new(),
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
            worker_config_encryption_key: String::new(),
            relay_secret_master_key: String::new(),
            session_ttl_seconds: 7 * 24 * 60 * 60,
            local_session_max_entries: 10_000,
            shutdown_drain_seconds: 120,
            mcp_warmup: McpWarmupMode::All,
            max_upstream_response_bytes: 64 * 1024 * 1024,
            max_raw_response_capture_bytes: 4 * 1024 * 1024,
            max_response_text_capture_bytes: 1024 * 1024,
            endpoint_model_cache_ttl_seconds: 300,
            mcp_allowed_origins: Vec::new(),
            mcp_oauth_redirect_base: String::new(),
        }
    }
}

impl WorkerConfig {
    /// Effective worker configuration encryption key with explicit
    /// precedence: a non-empty current `WORKER_CONFIG_ENCRYPTION_KEY` wins
    /// over the legacy `RELAY_SECRET_MASTER_KEY`; when both are empty the
    /// caller falls back to the persisted or generated key file.
    pub fn effective_encryption_key(&self) -> &str {
        if !self.worker_config_encryption_key.trim().is_empty() {
            &self.worker_config_encryption_key
        } else {
            &self.relay_secret_master_key
        }
    }

    /// Stable worker identity shared through the worker registry. A configured
    /// value wins; otherwise the host and admin bind keep two local processes
    /// distinct without introducing a machine-specific setting.
    pub fn resolved_worker_identity(&self) -> String {
        let hostname = std::env::var("HOSTNAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown-host".to_string());
        resolve_worker_identity(&self.worker_identity, &hostname, &self.admin_bind)
    }

    pub(crate) fn storage_backend(&self) -> StorageBackend {
        if self.database_url.trim().is_empty() {
            StorageBackend::Sqlite
        } else {
            StorageBackend::Postgres
        }
    }

    /// PostgreSQL personal MCP OAuth needs an explicitly configured shared
    /// encryption key so every worker can decrypt the same grants; the
    /// persisted file-local fallback must never be silently used for it.
    pub(crate) fn mcp_oauth_shared_key_missing(&self) -> bool {
        self.storage_backend().is_postgres() && self.effective_encryption_key().trim().is_empty()
    }
}

pub fn normalize_relay_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

/// Resolve the registry identity from an explicit name or the host plus admin
/// bind. The fallback keeps two local processes distinct without a new
/// machine-specific setting; operators of several hosts behind one database
/// should set an explicit `worker.worker_identity`.
pub(crate) fn resolve_worker_identity(
    configured: &str,
    hostname: &str,
    admin_bind: &str,
) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    format!("{hostname}:{}", admin_bind.trim())
}

#[cfg(test)]
mod tests {
    use super::{WorkerConfig, resolve_worker_identity};

    const KEY_A: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";
    const KEY_B: &str = "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=";

    #[test]
    fn deserializing_both_encryption_key_names_does_not_conflict() {
        // Both env/config names can be present at once (for example a legacy
        // .env plus a new override); this must not raise a duplicate-field
        // deserialization error.
        let json = format!(
            r#"{{"worker_config_encryption_key":"{KEY_A}","relay_secret_master_key":"{KEY_B}"}}"#
        );
        let config: WorkerConfig = serde_json::from_str(&json).expect("both key names parse");
        assert_eq!(config.worker_config_encryption_key, KEY_A);
        assert_eq!(config.relay_secret_master_key, KEY_B);
    }

    #[test]
    fn shutdown_drain_default_covers_a_slow_response_round() {
        assert_eq!(WorkerConfig::default().shutdown_drain_seconds, 120);
    }

    #[test]
    fn effective_encryption_key_prefers_current_over_legacy() {
        let config = WorkerConfig {
            worker_config_encryption_key: KEY_A.to_string(),
            relay_secret_master_key: KEY_B.to_string(),
            ..WorkerConfig::default()
        };
        assert_eq!(config.effective_encryption_key(), KEY_A);

        let config = WorkerConfig {
            relay_secret_master_key: KEY_B.to_string(),
            ..WorkerConfig::default()
        };
        assert_eq!(config.effective_encryption_key(), KEY_B);

        let config = WorkerConfig::default();
        assert_eq!(config.effective_encryption_key(), "");
    }

    #[test]
    fn mcp_oauth_requires_a_shared_key_only_for_postgres() {
        let sqlite = WorkerConfig::default();
        assert!(!sqlite.mcp_oauth_shared_key_missing());

        let postgres = WorkerConfig {
            database_url: "postgres://localhost/prompt_ferry".to_string(),
            ..WorkerConfig::default()
        };
        assert!(postgres.mcp_oauth_shared_key_missing());

        let postgres_current = WorkerConfig {
            database_url: "postgres://localhost/prompt_ferry".to_string(),
            worker_config_encryption_key: KEY_A.to_string(),
            ..WorkerConfig::default()
        };
        assert!(!postgres_current.mcp_oauth_shared_key_missing());

        let postgres_legacy = WorkerConfig {
            database_url: "postgres://localhost/prompt_ferry".to_string(),
            relay_secret_master_key: KEY_B.to_string(),
            ..WorkerConfig::default()
        };
        assert!(!postgres_legacy.mcp_oauth_shared_key_missing());

        let postgres_blank_current = WorkerConfig {
            database_url: "postgres://localhost/prompt_ferry".to_string(),
            worker_config_encryption_key: "   ".to_string(),
            ..WorkerConfig::default()
        };
        assert!(postgres_blank_current.mcp_oauth_shared_key_missing());
    }
    #[test]
    fn worker_identity_prefers_the_configured_name() {
        assert_eq!(
            resolve_worker_identity("worker-a", "host-a", "127.0.0.1:8789"),
            "worker-a"
        );
    }

    #[test]
    fn worker_identity_falls_back_to_host_and_admin_bind() {
        assert_eq!(
            resolve_worker_identity("   ", "host-a", "127.0.0.1:8789"),
            "host-a:127.0.0.1:8789"
        );
        assert_ne!(
            resolve_worker_identity("", "host-a", "127.0.0.1:8789"),
            resolve_worker_identity("", "host-a", "127.0.0.1:8791")
        );
    }
}
