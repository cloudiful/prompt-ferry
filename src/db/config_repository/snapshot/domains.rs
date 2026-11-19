//! Per-domain record shapes of the configuration snapshot.
//!
//! Every struct here is plain serializable data; the readers in
//! `snapshot_postgres`/`snapshot_sqlite` fill them and the manifest in
//! [`super::manifest`] summarizes them. Secret-shaped fields are `Option` so a
//! record can say "the source had nothing recoverable" instead of inventing a
//! value.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::db::McpOAuthGrantSnapshot;

/// Whether a secret-shaped field survived on the source backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretRecovery {
    /// The original value was read and is present in the archive.
    Recovered,
    /// Only a derived hash survived; the original value is unrecoverable and
    /// an import must keep the hash rather than invent a value.
    HashOnly,
    /// Nothing is stored for this field.
    Absent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSnapshot {
    pub user_id: i64,
    pub login_name: String,
    pub display_name: String,
    /// Argon2id PHC string. Plaintext passwords are never exported.
    pub password_hash: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientKeySnapshot {
    pub key_id: Uuid,
    pub user_id: i64,
    pub key_prefix: String,
    pub label: String,
    pub enabled: bool,
    /// `None` when the source backend does not expose a per-key timestamp.
    pub created_at: Option<DateTime<Utc>>,
    pub secret_state: SecretRecovery,
    /// Present when the source backend stored the original key material.
    pub secret: Option<String>,
    /// Present when the source backend stored (or the secret doubles as) the
    /// lookup hash.
    pub key_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointApiKeySnapshot {
    pub key_id: Uuid,
    pub key_label: String,
    pub position: i32,
    pub enabled: bool,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointOAuthSnapshot {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointSnapshot {
    pub endpoint_id: Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: String,
    pub provider_region: Option<String>,
    pub service_tier: Option<String>,
    pub base_url: String,
    pub native_api: String,
    pub native_api_source: String,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
    pub active_windows: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Legacy single-key column; also carried inside `api_keys`.
    pub api_key: Option<String>,
    pub api_keys: Vec<EndpointApiKeySnapshot>,
    pub proxy_url: Option<String>,
    pub admin_api_key: Option<String>,
    pub oauth: Option<EndpointOAuthSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRouteTargetSnapshot {
    pub target_id: Uuid,
    pub endpoint_id: Uuid,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub native_api: String,
    pub proxy_url_override: Option<String>,
    pub active_windows: Option<String>,
    pub dev_system_normalize: bool,
    pub thinking_downgrade_enabled: bool,
    pub thinking_effort_override: Option<String>,
    pub compact_mode: String,
    pub service_tier: Option<String>,
    /// Issue #1032: nullable positive embedding dimension override. Legacy
    /// archives omit it, so it deserializes as inherit (`None`).
    #[serde(default)]
    pub embedding_dimensions_override: Option<i32>,
    /// Manual new-session preference. Legacy archives omit it, so
    /// it deserializes as automatic (`false`).
    #[serde(default)]
    pub prefer_new_sessions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRouteSnapshot {
    pub rule_id: Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: String,
    pub enabled: bool,
    /// `None` when the source backend does not expose route timestamps.
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub targets: Vec<ModelRouteTargetSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSnapshot {
    pub server_id: Uuid,
    pub source_endpoint_id: Option<Uuid>,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub aggregate_naming_mode: String,
    pub transport: String,
    pub provider_kind: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Value,
    pub env_json: Value,
    pub http_headers_json: Value,
    pub auth_mode: String,
    pub basic_username: Option<String>,
    pub basic_password: Option<String>,
    pub proxy_url: Option<String>,
    pub tool_filter_mode: String,
    pub allowed_tools: Value,
    pub disabled_tools: Value,
    pub disabled_resources: Value,
    pub enabled: bool,
    pub timeout_ms: i32,
    pub lifecycle_policy: String,
    pub lifecycle_manual_protocol_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One archived MCP credential. The retired cost/strict-mode fields of an
/// older payload format are read by the bounded legacy reader and never reach
/// this record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCredentialSnapshot {
    pub credential_id: Uuid,
    pub server_id: Uuid,
    pub credential_label: String,
    pub secret: String,
    pub position: i32,
    pub enabled: bool,
    pub provider_kind: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingSnapshot {
    pub key: String,
    pub version: i64,
    pub value: Value,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRedactionConfigSnapshot {
    pub user_id: i64,
    pub config: Value,
}

/// Explicit per-user endpoint selection.
///
/// A row with `endpoint_id = None` means "use the routing default" and is
/// distinct from the absence of a row: both survive a round trip. The table's
/// `updated_at` is bookkeeping for the routing/config-revision cache, not
/// functional configuration, so it does not travel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserEndpointSettingSnapshot {
    pub user_id: i64,
    pub endpoint_id: Option<Uuid>,
}

/// The domain records only. Serializing this struct is what the manifest
/// fingerprint covers, so the fingerprint is stable across re-encodes.
///
/// This is the current (format 3) shape: ten portable domains. Deployment
/// state — the shared Valkey coordination row, the raw object-store
/// configuration and other internal settings — never travels here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDomains {
    pub users: Vec<UserSnapshot>,
    pub client_keys: Vec<ClientKeySnapshot>,
    pub endpoints: Vec<EndpointSnapshot>,
    pub model_routes: Vec<ModelRouteSnapshot>,
    pub mcp_servers: Vec<McpServerSnapshot>,
    pub mcp_credentials: Vec<McpCredentialSnapshot>,
    pub settings: Vec<SettingSnapshot>,
    pub user_redaction_configs: Vec<UserRedactionConfigSnapshot>,
    /// Personal MCP OAuth grants, decrypted for travel inside the encrypted
    /// archive. A format-2 archive written before this domain existed
    /// deserializes as an empty set and restores no grant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_oauth: Vec<McpOAuthGrantSnapshot>,
    /// Explicit per-user endpoint selections. Legacy archives omit the field,
    /// which deserializes as "no explicit selection".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_endpoint_settings: Vec<UserEndpointSettingSnapshot>,
}
