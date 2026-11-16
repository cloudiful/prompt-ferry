use anyhow::anyhow;

use crate::{
    config::{BridgeEncryptionMode, TlsMode, WorkerConfig, normalize_relay_url},
    tls,
};

#[derive(Clone)]
pub(super) struct RelayConnectionConfig {
    pub(super) relay_key: String,
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

/// Resolve every local relay target this worker dials.
///
/// Connection targets are always the worker's own `worker.relay_urls` list; no
/// database row or admin surface adds, removes or edits them. An empty list is
/// a valid state: the worker stays up without a bridge and never falls back to
/// another role.
pub(super) fn relay_connection_configs(
    config: &WorkerConfig,
) -> anyhow::Result<Vec<RelayConnectionConfig>> {
    config
        .relay_urls
        .iter()
        .map(|relay_url| relay_connection_config(config, relay_url))
        .collect()
}

pub(super) fn relay_connection_config(
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

fn optional_file_pem(path: &str) -> anyhow::Result<Option<String>> {
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tls::read_pem_file(path)?))
    }
}

pub(super) fn first_relay_connection_config(
    config: &WorkerConfig,
) -> anyhow::Result<RelayConnectionConfig> {
    relay_connection_configs(config)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("at least one relay URL is required"))
}
