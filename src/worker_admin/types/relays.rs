use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    config::{BridgeEncryptionMode, TlsMode, normalize_relay_url},
    db::{ManagedRelayRow, ManagedRelayRuntimeStatus},
};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManagedRelay {
    pub relay_id: Uuid,
    pub name: String,
    pub relay_url: String,
    pub enabled: bool,
    pub tls_mode: TlsMode,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub has_relay_ca: bool,
    pub has_client_cert: bool,
    pub has_client_key: bool,
    pub has_bridge_key: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_snapshot_version: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ManagedRelay {
    pub fn from_parts(row: ManagedRelayRow, runtime: ManagedRelayRuntimeStatus) -> Self {
        let tls_mode = row.tls_mode();
        let bridge_encryption_mode = row.bridge_encryption_mode();
        let has_relay_ca = row.relay_ca_envelope().is_some();
        let has_client_cert = row.client_cert_envelope().is_some();
        let has_client_key = row.client_key_envelope().is_some();
        let has_bridge_key = row.bridge_encryption_key_envelope().is_some();
        Self {
            relay_id: row.relay_id,
            name: row.name,
            relay_url: row.relay_url,
            enabled: row.enabled,
            tls_mode,
            bridge_encryption_mode,
            has_relay_ca,
            has_client_cert,
            has_client_key,
            has_bridge_key,
            connected: runtime.connected,
            last_error: runtime.last_error,
            last_connected_at: runtime.last_connected_at,
            last_disconnected_at: runtime.last_disconnected_at,
            last_snapshot_version: runtime.last_snapshot_version,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManagedRelayListResponse {
    pub relays: Vec<ManagedRelay>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ManagedRelayRequest {
    pub name: String,
    pub relay_url: String,
    pub enabled: Option<bool>,
    pub tls_mode: TlsMode,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub relay_ca_pem: Option<ManagedRelaySecretPatch>,
    pub client_cert_pem: Option<ManagedRelaySecretPatch>,
    pub client_key_pem: Option<ManagedRelaySecretPatch>,
    pub bridge_encryption_key: Option<ManagedRelaySecretPatch>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ManagedRelayPatchRequest {
    pub name: Option<String>,
    pub relay_url: Option<String>,
    pub enabled: Option<bool>,
    pub tls_mode: Option<TlsMode>,
    pub bridge_encryption_mode: Option<BridgeEncryptionMode>,
    pub relay_ca_pem: Option<ManagedRelaySecretPatch>,
    pub client_cert_pem: Option<ManagedRelaySecretPatch>,
    pub client_key_pem: Option<ManagedRelaySecretPatch>,
    pub bridge_encryption_key: Option<ManagedRelaySecretPatch>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum ManagedRelaySecretPatch {
    Keep,
    Clear,
    Replace { value: String },
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManagedRelayStatus {
    pub relay_id: Uuid,
    pub relay_url: String,
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_snapshot_version: Option<i64>,
}

impl ManagedRelayRequest {
    pub fn validate_create(&self) -> Result<(), String> {
        validate_name(&self.name)?;
        validate_relay_url(&self.relay_url, self.tls_mode)?;
        validate_create_secret_mode(&self.relay_ca_pem, "relay_ca_pem")?;
        validate_create_secret_mode(&self.client_cert_pem, "client_cert_pem")?;
        validate_create_secret_mode(&self.client_key_pem, "client_key_pem")?;
        validate_create_secret_mode(&self.bridge_encryption_key, "bridge_encryption_key")?;
        Ok(())
    }
}

impl ManagedRelayPatchRequest {
    pub fn validate_patch(&self, current_tls_mode: TlsMode) -> Result<(), String> {
        if let Some(name) = self.name.as_ref() {
            validate_name(name)?;
        }
        if let Some(relay_url) = self.relay_url.as_ref() {
            validate_relay_url(relay_url, self.tls_mode.unwrap_or(current_tls_mode))?;
        }
        Ok(())
    }
}

fn validate_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err("name is required".to_string())
    } else {
        Ok(())
    }
}

fn validate_relay_url(value: &str, tls_mode: TlsMode) -> Result<(), String> {
    let value = normalize_relay_url(value);
    if value.is_empty() {
        return Err("relay_url is required".to_string());
    }
    match tls_mode {
        TlsMode::Off if value.starts_with("ws://") => Ok(()),
        TlsMode::Off => Err("relay_url must use ws:// when tls_mode=off".to_string()),
        TlsMode::Server | TlsMode::Mtls if value.starts_with("wss://") => Ok(()),
        TlsMode::Server | TlsMode::Mtls => Err(format!(
            "relay_url must use wss:// when tls_mode={}",
            tls_mode.as_str()
        )),
    }
}

fn validate_create_secret_mode(
    patch: &Option<ManagedRelaySecretPatch>,
    field_name: &str,
) -> Result<(), String> {
    match patch {
        Some(ManagedRelaySecretPatch::Replace { .. }) | None => Ok(()),
        Some(ManagedRelaySecretPatch::Keep) => {
            Err(format!("{field_name} cannot use keep on create"))
        }
        Some(ManagedRelaySecretPatch::Clear) => {
            Err(format!("{field_name} cannot use clear on create"))
        }
    }
}
