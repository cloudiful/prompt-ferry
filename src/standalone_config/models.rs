use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{BridgeEncryptionMode, NativeApi, NativeApiSource, TlsMode};

#[derive(Debug)]
pub enum StandaloneConfigError {
    InvalidInput {
        field: &'static str,
        message: String,
    },
    MissingSecretManager {
        operation: &'static str,
    },
    UnsupportedSchemaVersion {
        found: i64,
        supported: i64,
    },
    CorruptDatabase(String),
    Database(sqlx::Error),
    Serialization(serde_json::Error),
    Encryption(anyhow::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for StandaloneConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { field, message } => {
                write!(formatter, "invalid {field}: {message}")
            }
            Self::MissingSecretManager { operation } => write!(
                formatter,
                "a RelaySecretManager is required for standalone configuration {operation}"
            ),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                formatter,
                "unsupported standalone SQLite schema version {found}; supported version is {supported}"
            ),
            Self::CorruptDatabase(message) => {
                write!(formatter, "corrupt standalone SQLite database: {message}")
            }
            Self::Database(error) => write!(formatter, "standalone SQLite database error: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "invalid standalone setting JSON: {error}")
            }
            Self::Encryption(error) => {
                write!(formatter, "standalone secret encryption error: {error}")
            }
            Self::Io(error) => write!(formatter, "standalone SQLite file error: {error}"),
        }
    }
}

impl std::error::Error for StandaloneConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::Encryption(error) => Some(error.root_cause()),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StandaloneConfigError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for StandaloneConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl From<anyhow::Error> for StandaloneConfigError {
    fn from(error: anyhow::Error) -> Self {
        Self::Encryption(error)
    }
}

impl From<std::io::Error> for StandaloneConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, StandaloneConfigError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProvider {
    Generic,
    Minimax,
}

impl EndpointProvider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Minimax => "minimax",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "generic" => Ok(Self::Generic),
            "minimax" => Ok(Self::Minimax),
            _ => Err(StandaloneConfigError::CorruptDatabase(format!(
                "unknown endpoint provider {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRegion {
    Cn,
    Global,
}

impl EndpointRegion {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Global => "global",
        }
    }

    pub(crate) fn parse(value: Option<&str>) -> Result<Option<Self>> {
        match value {
            None => Ok(None),
            Some("cn") => Ok(Some(Self::Cn)),
            Some("global") => Ok(Some(Self::Global)),
            Some(value) => Err(StandaloneConfigError::CorruptDatabase(format!(
                "unknown endpoint region {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteScope {
    Admin,
    User,
}

impl RouteScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "admin" => Ok(Self::Admin),
            "user" => Ok(Self::User),
            _ => Err(StandaloneConfigError::CorruptDatabase(format!(
                "unknown route scope {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    ClientKeyRendezvous,
    ResponsesSessionAffinity,
}

impl RoutingStrategy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ClientKeyRendezvous => "client_key_rendezvous",
            Self::ResponsesSessionAffinity => "responses_session_affinity",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "client_key_rendezvous" => Ok(Self::ClientKeyRendezvous),
            "responses_session_affinity" => Ok(Self::ResponsesSessionAffinity),
            _ => Err(StandaloneConfigError::CorruptDatabase(format!(
                "unknown routing strategy {value:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationPolicy {
    ForcePassthrough,
    ForceReplay,
}

impl ContinuationPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ForcePassthrough => "force_passthrough",
            Self::ForceReplay => "force_replay",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "force_passthrough" => Ok(Self::ForcePassthrough),
            "force_replay" => Ok(Self::ForceReplay),
            _ => Err(StandaloneConfigError::CorruptDatabase(format!(
                "unknown continuation policy {value:?}"
            ))),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedRelayConfig {
    pub relay_id: Uuid,
    pub name: String,
    pub relay_url: String,
    pub enabled: bool,
    pub tls_mode: TlsMode,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub relay_ca_pem: Option<String>,
    pub client_cert_pem: Option<String>,
    pub client_key_pem: Option<String>,
    pub bridge_encryption_key: Option<String>,
}

impl fmt::Debug for ManagedRelayConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedRelayConfig")
            .field("relay_id", &self.relay_id)
            .field("name", &self.name)
            .field("relay_url", &self.relay_url)
            .field("enabled", &self.enabled)
            .field("tls_mode", &self.tls_mode)
            .field("bridge_encryption_mode", &self.bridge_encryption_mode)
            .field(
                "relay_ca_pem",
                &redacted_optional_secret(self.relay_ca_pem.as_deref()),
            )
            .field(
                "client_cert_pem",
                &redacted_optional_secret(self.client_cert_pem.as_deref()),
            )
            .field(
                "client_key_pem",
                &redacted_optional_secret(self.client_key_pem.as_deref()),
            )
            .field(
                "bridge_encryption_key",
                &redacted_optional_secret(self.bridge_encryption_key.as_deref()),
            )
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointApiKeyConfig {
    pub key_id: Uuid,
    pub endpoint_id: Uuid,
    pub key_label: String,
    pub api_key: String,
    pub position: i32,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl fmt::Debug for EndpointApiKeyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointApiKeyConfig")
            .field("key_id", &self.key_id)
            .field("key_label", &self.key_label)
            .field("api_key", &redacted_secret(&self.api_key))
            .field("position", &self.position)
            .field("enabled", &self.enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEndpointConfig {
    pub endpoint_id: Uuid,
    pub name: String,
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    pub base_url: String,
    pub native_api: NativeApi,
    pub native_api_source: NativeApiSource,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub api_key: String,
    pub api_keys: Vec<EndpointApiKeyConfig>,
}

impl fmt::Debug for ProviderEndpointConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderEndpointConfig")
            .field("endpoint_id", &self.endpoint_id)
            .field("name", &self.name)
            .field("provider", &self.provider)
            .field("provider_region", &self.provider_region)
            .field("base_url", &self.base_url)
            .field("native_api", &self.native_api)
            .field("native_api_source", &self.native_api_source)
            .field("key_lb_enabled", &self.key_lb_enabled)
            .field("enabled", &self.enabled)
            .field("mcp_enabled", &self.mcp_enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("api_key", &redacted_secret(&self.api_key))
            .field("api_keys", &self.api_keys)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRouteTargetConfig {
    pub target_id: Uuid,
    pub endpoint_id: Uuid,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: ContinuationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRouteConfig {
    pub rule_id: Uuid,
    pub scope: RouteScope,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: RoutingStrategy,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub targets: Vec<ModelRouteTargetConfig>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientKeyConfig {
    pub key_id: Uuid,
    pub user_id: i64,
    pub key_prefix: String,
    pub label: String,
    pub secret: String,
    pub enabled: bool,
}

impl fmt::Debug for ClientKeyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientKeyConfig")
            .field("key_id", &self.key_id)
            .field("user_id", &self.user_id)
            .field("key_prefix", &self.key_prefix)
            .field("label", &self.label)
            .field("secret", &redacted_secret(&self.secret))
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingConfig {
    pub key: String,
    pub version: i64,
    pub value: serde_json::Value,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StandaloneConfig {
    pub relays: Vec<ManagedRelayConfig>,
    pub endpoints: Vec<ProviderEndpointConfig>,
    pub routes: Vec<ModelRouteConfig>,
    pub client_keys: Vec<ClientKeyConfig>,
    pub settings: Vec<SettingConfig>,
}

impl fmt::Debug for StandaloneConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StandaloneConfig")
            .field("relays", &self.relays)
            .field("endpoints", &self.endpoints)
            .field("routes", &self.routes)
            .field("client_keys", &self.client_keys)
            .field("settings", &self.settings)
            .finish()
    }
}

#[derive(Clone)]
pub struct BootstrapSeed {
    pub relay_urls: Vec<String>,
    pub tls_mode: TlsMode,
    pub relay_ca_pem: Option<String>,
    pub client_cert_pem: Option<String>,
    pub client_key_pem: Option<String>,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub bridge_encryption_key: Option<String>,
    pub upstream_base_url: String,
    pub upstream_api_key: String,
    pub upstream_native_api: NativeApi,
}

impl fmt::Debug for BootstrapSeed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BootstrapSeed")
            .field("relay_urls", &self.relay_urls)
            .field("tls_mode", &self.tls_mode)
            .field(
                "relay_ca_pem",
                &redacted_optional_secret(self.relay_ca_pem.as_deref()),
            )
            .field(
                "client_cert_pem",
                &redacted_optional_secret(self.client_cert_pem.as_deref()),
            )
            .field(
                "client_key_pem",
                &redacted_optional_secret(self.client_key_pem.as_deref()),
            )
            .field("bridge_encryption_mode", &self.bridge_encryption_mode)
            .field(
                "bridge_encryption_key",
                &redacted_optional_secret(self.bridge_encryption_key.as_deref()),
            )
            .field("upstream_base_url", &self.upstream_base_url)
            .field("upstream_api_key", &redacted_secret(&self.upstream_api_key))
            .field("upstream_native_api", &self.upstream_native_api)
            .finish()
    }
}

fn redacted_secret(value: &str) -> String {
    format!("[REDACTED; {} bytes]", value.len())
}

fn redacted_optional_secret(value: Option<&str>) -> Option<String> {
    value.map(redacted_secret)
}

/// Compact, durable representation of a standalone usage summary.
///
/// This DTO mirrors `StandaloneUsageSummary` and is the storage boundary for
/// the SQLite request ledger introduced in Phase 1A. Phase 1C-a extends it
/// with the non-secret request metadata fields carried by
/// `RequestRecordCreate` so a later Admin query surface can read the same
/// routing/context columns without re-fetching them. Raw request/response
/// bodies, encrypted upstream sessions, billing snapshots, approvals, and
/// quota state remain intentionally absent.
///
/// The struct owns its own primitive representation (UUID text, RFC3339
/// timestamps, integer booleans, JSON strings) so the standalone-config
/// storage layer never needs to depend on the higher-level usage logger.
/// Conversion to and from the runtime summary type lives in
/// `crate::usage::logging::models`.
#[derive(Clone, PartialEq)]
pub struct StandaloneUsageSummaryRecord {
    pub request_id: Uuid,
    pub event_kind: String,
    pub category: String,
    pub state: String,
    pub path: String,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub status: Option<i32>,
    pub ok: Option<bool>,
    pub duration_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_key_id: Option<Uuid>,
    pub model_route_rule_id: Option<Uuid>,
    pub mcp_server_id: Option<Uuid>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub failure_family: Option<String>,
    pub redaction_applied: bool,
    pub redaction_findings_count: i32,
    pub redaction_replacements_count: i32,
    pub redaction_types: Vec<String>,
    pub redaction_fields: Vec<String>,
    pub route_selection_reason: String,
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub request_user_agent: Option<String>,
    pub endpoint_key_label: Option<String>,
    pub mcp_server_name: Option<String>,
    pub mcp_protocol_method: Option<String>,
    pub mcp_operation_name: Option<String>,
    pub http_request_content_encoding: Option<String>,
    pub http_request_compressed: bool,
    pub http_request_compressed_bytes: Option<i64>,
    pub http_request_decompressed_bytes: Option<i64>,
    pub http_request_compression_ratio: Option<f64>,
    pub conversation_source: String,
    pub client_installation_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub provider_conversation_key: Option<String>,
    pub request_storage_mode: String,
    pub error_message: Option<String>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub upstream_redaction_enabled: bool,
    pub response_capture_truncated: bool,
}

/// Durable standalone replay snapshot for a single conversation.
///
/// Mirrors the columns of `standalone_replay_snapshots` introduced by
/// migration 0008. Only the latest checkpoint per conversation is
/// persisted; lower or equal sequence numbers are silently dropped by
/// the monotonic upsert SQL. `prompt_refs_json` is the same shape as
/// the PostgreSQL replay snapshot column and the in-memory
/// `ReplaySnapshotValue::prompt_refs` (role + block-hash references),
/// not raw request or response bodies.
#[derive(Clone, PartialEq, Eq)]
pub struct StandaloneReplaySnapshotRecord {
    pub conversation_id: Uuid,
    pub base_event_id: i64,
    pub conversation_seq: i32,
    pub prompt_refs_json: String,
    pub ref_count: i32,
    pub byte_size: i32,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl fmt::Debug for StandaloneReplaySnapshotRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StandaloneReplaySnapshotRecord")
            .field("conversation_id", &self.conversation_id)
            .field("base_event_id", &self.base_event_id)
            .field("conversation_seq", &self.conversation_seq)
            .field("prompt_refs_json", &self.prompt_refs_json)
            .field("ref_count", &self.ref_count)
            .field("byte_size", &self.byte_size)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// Outcome of a monotonic upsert against `standalone_replay_snapshots`.
/// The store distinguishes "applied" from "skipped" without a separate
/// read-then-compare transaction so the runtime can warn about repeated
/// regressions without losing the existing snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySnapshotUpsertOutcome {
    /// No existing snapshot for the conversation; a fresh row was
    /// inserted.
    Inserted,
    /// The incoming snapshot was strictly newer by the
    /// `(conversation_seq, base_event_id)` ordering and replaced the
    /// stored row.
    Updated,
    /// The incoming snapshot would regress the stored row by the
    /// ordering, so the existing row was preserved unchanged.
    Skipped,
}

impl ReplaySnapshotUpsertOutcome {
    pub fn applied(self) -> bool {
        matches!(self, Self::Inserted | Self::Updated)
    }
}

impl fmt::Debug for StandaloneUsageSummaryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StandaloneUsageSummaryRecord")
            .field("request_id", &self.request_id)
            .field("event_kind", &self.event_kind)
            .field("category", &self.category)
            .field("state", &self.state)
            .field("path", &self.path)
            .field("recorded_at", &self.recorded_at)
            .field("status", &self.status)
            .field("ok", &self.ok)
            .field("duration_ms", &self.duration_ms)
            .field("ttft_ms", &self.ttft_ms)
            .field("model", &self.model)
            .field("requested_model", &self.requested_model)
            .field("upstream_model", &self.upstream_model)
            .field("endpoint_id", &self.endpoint_id)
            .field("endpoint_key_id", &self.endpoint_key_id)
            .field("model_route_rule_id", &self.model_route_rule_id)
            .field("mcp_server_id", &self.mcp_server_id)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("total_tokens", &self.total_tokens)
            .field("cached_tokens", &self.cached_tokens)
            .field("cache_read_tokens", &self.cache_read_tokens)
            .field("cache_write_tokens", &self.cache_write_tokens)
            .field("error_code", &self.error_code)
            .field("failure_family", &self.failure_family)
            .field("redaction_applied", &self.redaction_applied)
            .field("redaction_findings_count", &self.redaction_findings_count)
            .field(
                "redaction_replacements_count",
                &self.redaction_replacements_count,
            )
            .field("redaction_types", &self.redaction_types)
            .field("redaction_fields", &self.redaction_fields)
            .field("route_selection_reason", &self.route_selection_reason)
            .field("user_id", &self.user_id)
            .field("client_key_id", &self.client_key_id)
            .field("client_key_label", &self.client_key_label)
            .field("request_user_agent", &self.request_user_agent)
            .field("endpoint_key_label", &self.endpoint_key_label)
            .field("mcp_server_name", &self.mcp_server_name)
            .field("mcp_protocol_method", &self.mcp_protocol_method)
            .field("mcp_operation_name", &self.mcp_operation_name)
            .field(
                "http_request_content_encoding",
                &self.http_request_content_encoding,
            )
            .field("http_request_compressed", &self.http_request_compressed)
            .field(
                "http_request_compressed_bytes",
                &self.http_request_compressed_bytes,
            )
            .field(
                "http_request_decompressed_bytes",
                &self.http_request_decompressed_bytes,
            )
            .field(
                "http_request_compression_ratio",
                &self.http_request_compression_ratio,
            )
            .field("conversation_source", &self.conversation_source)
            .field("client_installation_id", &self.client_installation_id)
            .field("provider_response_id", &self.provider_response_id)
            .field("provider_conversation_key", &self.provider_conversation_key)
            .field("request_storage_mode", &self.request_storage_mode)
            .field("error_message", &self.error_message)
            .field(
                "request_has_previous_response_id",
                &self.request_has_previous_response_id,
            )
            .field(
                "request_previous_response_id",
                &self.request_previous_response_id,
            )
            .field(
                "request_previous_response_parent_found",
                &self.request_previous_response_parent_found,
            )
            .field("request_conversation_key", &self.request_conversation_key)
            .field(
                "request_conversation_parent_found",
                &self.request_conversation_parent_found,
            )
            .field(
                "upstream_redaction_enabled",
                &self.upstream_redaction_enabled,
            )
            .field(
                "response_capture_truncated",
                &self.response_capture_truncated,
            )
            .finish()
    }
}
