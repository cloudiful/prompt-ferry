use super::*;
use super::{
    relay_secrets::{encrypt_create_secret, resolve_secret_patch},
    relay_validation::{FinalRelayConfig, ensure_unique_relay_url, validate_final_relay_config},
};
use crate::config::normalize_relay_url;

pub(super) async fn resolve_create_relay_input(
    state: &AdminState,
    body: ManagedRelayRequest,
) -> Result<db::ManagedRelayInput, Box<Response>> {
    let relay_url = normalize_relay_url(&body.relay_url);
    ensure_unique_relay_url(state, &relay_url, None).await?;
    let manager = state
        .relay_secret_manager()
        .map_err(|err| Box::new(internal(state, err)))?;
    let relay_ca = encrypt_create_secret(manager, body.relay_ca_pem, "relay_ca_pem")?;
    let client_cert = encrypt_create_secret(manager, body.client_cert_pem, "client_cert_pem")?;
    let client_key = encrypt_create_secret(manager, body.client_key_pem, "client_key_pem")?;
    let bridge_encryption_key =
        encrypt_create_secret(manager, body.bridge_encryption_key, "bridge_encryption_key")?;
    validate_final_relay_config(FinalRelayConfig {
        relay_url: &relay_url,
        tls_mode: body.tls_mode,
        bridge_encryption_mode: body.bridge_encryption_mode,
        has_client_cert: client_cert.is_some(),
        has_client_key: client_key.is_some(),
        has_bridge_key: bridge_encryption_key.is_some(),
        relay_ca_pem: decrypt_secret(manager, relay_ca.as_ref())?,
        client_cert_pem: decrypt_secret(manager, client_cert.as_ref())?,
        client_key_pem: decrypt_secret(manager, client_key.as_ref())?,
        bridge_encryption_key: decrypt_secret(manager, bridge_encryption_key.as_ref())?,
    })?;
    Ok(db::ManagedRelayInput {
        name: body.name.trim().to_string(),
        relay_url,
        enabled: body.enabled.unwrap_or(true),
        tls_mode: body.tls_mode,
        bridge_encryption_mode: body.bridge_encryption_mode,
        relay_ca,
        client_cert,
        client_key,
        bridge_encryption_key,
    })
}

pub(super) async fn resolve_update_relay_input(
    state: &AdminState,
    existing: crate::db::config_repository::UnifiedManagedRelay,
    body: ManagedRelayPatchRequest,
) -> Result<db::ManagedRelayInput, Box<Response>> {
    let manager = state
        .relay_secret_manager()
        .map_err(|err| Box::new(internal(state, err)))?;
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(existing.name.as_str())
        .to_string();
    let relay_url = normalize_relay_url(body.relay_url.as_deref().unwrap_or(&existing.relay_url));
    ensure_unique_relay_url(state, &relay_url, Some(existing.relay_id)).await?;
    let tls_mode = body.tls_mode.unwrap_or(existing.tls_mode);
    let bridge_encryption_mode = body
        .bridge_encryption_mode
        .unwrap_or(existing.bridge_encryption_mode);
    // When updating a relay, the previous secrets are already encrypted on
    // disk; treat any incoming patch as a Keep/Clear/Replace on top of the
    // stored envelopes. We pull the existing envelopes from the unified
    // repository's `get_managed_relay` call (the unified DTO only exposes
    // booleans) by falling back to the existing PostgreSQL row.
    let existing_secrets = load_existing_secrets(state, existing.relay_id).await?;
    let relay_ca = resolve_secret_patch(
        manager,
        body.relay_ca_pem,
        existing_secrets.as_ref().and_then(|s| s.relay_ca.clone()),
        "relay_ca_pem",
    )?;
    let client_cert = resolve_secret_patch(
        manager,
        body.client_cert_pem,
        existing_secrets
            .as_ref()
            .and_then(|s| s.client_cert.clone()),
        "client_cert_pem",
    )?;
    let client_key = resolve_secret_patch(
        manager,
        body.client_key_pem,
        existing_secrets.as_ref().and_then(|s| s.client_key.clone()),
        "client_key_pem",
    )?;
    let bridge_encryption_key = resolve_secret_patch(
        manager,
        body.bridge_encryption_key,
        existing_secrets.and_then(|s| s.bridge_key),
        "bridge_encryption_key",
    )?;
    validate_final_relay_config(FinalRelayConfig {
        relay_url: &relay_url,
        tls_mode,
        bridge_encryption_mode,
        has_client_cert: client_cert.is_some(),
        has_client_key: client_key.is_some(),
        has_bridge_key: bridge_encryption_key.is_some(),
        relay_ca_pem: decrypt_secret(manager, relay_ca.as_ref())?,
        client_cert_pem: decrypt_secret(manager, client_cert.as_ref())?,
        client_key_pem: decrypt_secret(manager, client_key.as_ref())?,
        bridge_encryption_key: decrypt_secret(manager, bridge_encryption_key.as_ref())?,
    })?;
    Ok(db::ManagedRelayInput {
        name,
        relay_url,
        enabled: body.enabled.unwrap_or(existing.enabled),
        tls_mode,
        bridge_encryption_mode,
        relay_ca,
        client_cert,
        client_key,
        bridge_encryption_key,
    })
}

async fn load_existing_secrets(
    state: &AdminState,
    relay_id: uuid::Uuid,
) -> Result<Option<crate::db::config_repository::ManagedRelaySecrets>, Box<Response>> {
    use crate::db::config_repository::relay_secrets_for_state;
    relay_secrets_for_state(state, relay_id)
        .await
        .map_err(|err| Box::new(internal(state, err)))
}

fn decrypt_secret(
    manager: &crate::relay_secrets::RelaySecretManager,
    secret: Option<&crate::relay_secrets::EncryptedSecretEnvelope>,
) -> Result<Option<String>, Box<Response>> {
    secret
        .map(|value| manager.decrypt(value))
        .transpose()
        .map_err(|err| Box::new(bad_request(&err.to_string())))
}
