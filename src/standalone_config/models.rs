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
