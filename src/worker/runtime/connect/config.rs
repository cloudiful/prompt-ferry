use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    config::{BridgeEncryptionMode, TlsMode, WorkerConfig, normalize_relay_url},
    tls, worker_admin,
};

#[derive(Clone)]
pub(super) struct RelayConnectionConfig {
    pub(super) relay_key: String,
    pub(super) relay_id: Option<Uuid>,
    pub(super) relay_url: String,
    pub(super) worker_token: String,
    pub(super) connect_timeout_seconds: u64,
    pub(super) tls_mode: TlsMode,
    pub(super) relay_ca_pem: Option<String>,
    pub(super) client_cert_pem: Option<String>,
    pub(super) client_key_pem: Option<String>,
    pub(super) bridge_encryption_mode: BridgeEncryptionMode,
    pub(super) bridge_encryption_key: String,
}

pub(super) fn simple_relay_connection_configs(
    config: &WorkerConfig,
) -> anyhow::Result<Vec<RelayConnectionConfig>> {
    config
        .relay_urls
        .iter()
        .map(|relay_url| simple_relay_connection_config(config, relay_url))
        .collect()
}

pub(super) async fn managed_relay_connection_config(
    config: &WorkerConfig,
    admin_state: &worker_admin::AdminState,
    relay: &crate::db::ManagedRelayRow,
) -> anyhow::Result<RelayConnectionConfig> {
    let manager = admin_state.relay_secret_manager()?;
    let relay_url = normalize_relay_url(&relay.relay_url);
    let tls_mode = relay.tls_mode();
    let bridge_encryption_mode = relay.bridge_encryption_mode();
    let relay_ca_pem = relay
        .relay_ca_envelope()
        .map(|value| manager.decrypt(&value))
        .transpose()?;
    let client_cert_pem = relay
        .client_cert_envelope()
        .map(|value| manager.decrypt(&value))
        .transpose()?;
    let client_key_pem = relay
        .client_key_envelope()
        .map(|value| manager.decrypt(&value))
        .transpose()?;
    let bridge_encryption_key = relay
        .bridge_encryption_key_envelope()
        .map(|value| manager.decrypt(&value))
        .transpose()?
        .unwrap_or_default();

    tls::validate_worker_relay_material(
        &relay_url,
        tls_mode,
        relay_ca_pem.as_deref(),
        client_cert_pem.as_deref(),
        client_key_pem.as_deref(),
    )?;
    crate::bridge_crypto::validate_settings(
        "worker",
        bridge_encryption_mode,
        &bridge_encryption_key,
    )?;

    Ok(RelayConnectionConfig {
        relay_key: relay.relay_id.to_string(),
        relay_id: Some(relay.relay_id),
        relay_url,
        worker_token: config.worker_token.clone(),
        connect_timeout_seconds: config.connect_timeout_seconds,
        tls_mode,
        relay_ca_pem,
        client_cert_pem,
        client_key_pem,
        bridge_encryption_mode,
        bridge_encryption_key,
    })
}

pub(super) fn simple_relay_connection_config(
    config: &WorkerConfig,
    relay_url: &str,
) -> anyhow::Result<RelayConnectionConfig> {
    let relay_url = normalize_relay_url(relay_url);
    let tls_mode = tls::worker_tls_mode(config, &relay_url)?;
    let relay_ca_pem = optional_file_pem(&config.relay_ca)?;
    let client_cert_pem = optional_file_pem(&config.client_cert)?;
    let client_key_pem = optional_file_pem(&config.client_key)?;
    tls::validate_worker_relay_material(
        &relay_url,
        tls_mode,
        relay_ca_pem.as_deref(),
        client_cert_pem.as_deref(),
        client_key_pem.as_deref(),
    )?;
    Ok(RelayConnectionConfig {
        relay_key: relay_url.clone(),
        relay_id: None,
        relay_url,
        worker_token: config.worker_token.clone(),
        connect_timeout_seconds: config.connect_timeout_seconds,
        tls_mode,
        relay_ca_pem,
        client_cert_pem,
        client_key_pem,
        bridge_encryption_mode: config.bridge_encryption_mode,
        bridge_encryption_key: config.bridge_encryption_key.clone(),
    })
}

pub(super) fn relay_fingerprint(relay: &crate::db::ManagedRelayRow) -> String {
    let mut digest = Sha256::new();
    digest.update(relay.name.as_bytes());
    digest.update(relay.relay_url.as_bytes());
    digest.update([relay.enabled as u8]);
    digest.update(relay.tls_mode.as_bytes());
    digest.update(relay.bridge_encryption_mode.as_bytes());
    for maybe_bytes in [
        relay.relay_ca_ciphertext.as_deref(),
        relay.relay_ca_nonce.as_deref(),
        relay.client_cert_ciphertext.as_deref(),
        relay.client_cert_nonce.as_deref(),
        relay.client_key_ciphertext.as_deref(),
        relay.client_key_nonce.as_deref(),
        relay.bridge_encryption_key_ciphertext.as_deref(),
        relay.bridge_encryption_key_nonce.as_deref(),
    ] {
        if let Some(bytes) = maybe_bytes {
            digest.update(bytes);
        }
        digest.update([0xff]);
    }
    for maybe_version in [
        relay.relay_ca_key_version,
        relay.client_cert_key_version,
        relay.client_key_key_version,
        relay.bridge_encryption_key_key_version,
    ] {
        digest.update(maybe_version.unwrap_or_default().to_be_bytes());
    }
    STANDARD.encode(digest.finalize())
}

fn optional_file_pem(path: &str) -> anyhow::Result<Option<String>> {
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tls::read_pem_file(path)?))
    }
}

pub(super) fn first_simple_relay_connection_config(
    config: &WorkerConfig,
) -> anyhow::Result<RelayConnectionConfig> {
    simple_relay_connection_configs(config)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("at least one relay URL is required"))
}
