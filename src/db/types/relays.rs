use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    config::{BridgeEncryptionMode, TlsMode},
    relay_secrets::EncryptedSecretEnvelope,
};

#[derive(Debug, Clone, FromRow)]
pub struct ManagedRelayRow {
    pub relay_id: Uuid,
    pub name: String,
    pub relay_url: String,
    pub enabled: bool,
    pub tls_mode: String,
    pub bridge_encryption_mode: String,
    pub relay_ca_ciphertext: Option<Vec<u8>>,
    pub relay_ca_nonce: Option<Vec<u8>>,
    pub relay_ca_key_version: Option<i16>,
    pub client_cert_ciphertext: Option<Vec<u8>>,
    pub client_cert_nonce: Option<Vec<u8>>,
    pub client_cert_key_version: Option<i16>,
    pub client_key_ciphertext: Option<Vec<u8>>,
    pub client_key_nonce: Option<Vec<u8>>,
    pub client_key_key_version: Option<i16>,
    pub bridge_encryption_key_ciphertext: Option<Vec<u8>>,
    pub bridge_encryption_key_nonce: Option<Vec<u8>>,
    pub bridge_encryption_key_key_version: Option<i16>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ManagedRelayRow {
    pub fn tls_mode(&self) -> TlsMode {
        serde_json::from_value(serde_json::Value::String(self.tls_mode.clone()))
            .unwrap_or(TlsMode::Off)
    }

    pub fn bridge_encryption_mode(&self) -> BridgeEncryptionMode {
        serde_json::from_value(serde_json::Value::String(
            self.bridge_encryption_mode.clone(),
        ))
        .unwrap_or(BridgeEncryptionMode::Off)
    }

    pub fn relay_ca_envelope(&self) -> Option<EncryptedSecretEnvelope> {
        envelope(
            self.relay_ca_ciphertext.clone(),
            self.relay_ca_nonce.clone(),
            self.relay_ca_key_version,
        )
    }

    pub fn client_cert_envelope(&self) -> Option<EncryptedSecretEnvelope> {
        envelope(
            self.client_cert_ciphertext.clone(),
            self.client_cert_nonce.clone(),
            self.client_cert_key_version,
        )
    }

    pub fn client_key_envelope(&self) -> Option<EncryptedSecretEnvelope> {
        envelope(
            self.client_key_ciphertext.clone(),
            self.client_key_nonce.clone(),
            self.client_key_key_version,
        )
    }

    pub fn bridge_encryption_key_envelope(&self) -> Option<EncryptedSecretEnvelope> {
        envelope(
            self.bridge_encryption_key_ciphertext.clone(),
            self.bridge_encryption_key_nonce.clone(),
            self.bridge_encryption_key_key_version,
        )
    }
}

#[derive(Debug, Clone)]
pub struct ManagedRelayInput {
    pub name: String,
    pub relay_url: String,
    pub enabled: bool,
    pub tls_mode: TlsMode,
    pub bridge_encryption_mode: BridgeEncryptionMode,
    pub relay_ca: Option<EncryptedSecretEnvelope>,
    pub client_cert: Option<EncryptedSecretEnvelope>,
    pub client_key: Option<EncryptedSecretEnvelope>,
    pub bridge_encryption_key: Option<EncryptedSecretEnvelope>,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ManagedRelayRuntimeStatus {
    pub connected: bool,
    pub last_error: Option<String>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_snapshot_version: Option<i64>,
}

fn envelope(
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    key_version: Option<i16>,
) -> Option<EncryptedSecretEnvelope> {
    Some(EncryptedSecretEnvelope {
        ciphertext: ciphertext?,
        nonce: nonce?,
        key_version: key_version?,
    })
}
