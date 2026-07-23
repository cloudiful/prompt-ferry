use super::*;
use super::{
    relay_secrets::{encrypt_create_secret, resolve_secret_patch},
    relay_validation::{ensure_unique_relay_url, validate_final_relay_config},
};
use crate::config::normalize_relay_url;

pub(super) async fn resolve_create_relay_input(
    state: &AdminState,
    body: ManagedRelayRequest,
) -> Result<db::ManagedRelayInput, Response> {
    let relay_url = normalize_relay_url(&body.relay_url);
    ensure_unique_relay_url(state, &relay_url, None).await?;
    let manager = state
        .relay_secret_manager()
        .map_err(|err| internal(state, err))?;
    let relay_ca = encrypt_create_secret(manager, body.relay_ca_pem, "relay_ca_pem")?;
    let client_cert = encrypt_create_secret(manager, body.client_cert_pem, "client_cert_pem")?;
    let client_key = encrypt_create_secret(manager, body.client_key_pem, "client_key_pem")?;
    let bridge_encryption_key =
        encrypt_create_secret(manager, body.bridge_encryption_key, "bridge_encryption_key")?;
    validate_final_relay_config(
        &relay_url,
        body.tls_mode,
        body.bridge_encryption_mode,
        client_cert.is_some(),
        client_key.is_some(),
        bridge_encryption_key.is_some(),
        decrypt_secret(manager, relay_ca.as_ref())?,
        decrypt_secret(manager, client_cert.as_ref())?,
        decrypt_secret(manager, client_key.as_ref())?,
        decrypt_secret(manager, bridge_encryption_key.as_ref())?,
    )?;
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
    existing: db::ManagedRelayRow,
    body: ManagedRelayPatchRequest,
) -> Result<db::ManagedRelayInput, Response> {
    let manager = state
        .relay_secret_manager()
        .map_err(|err| internal(state, err))?;
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(existing.name.as_str())
        .to_string();
    let relay_url = normalize_relay_url(body.relay_url.as_deref().unwrap_or(&existing.relay_url));
    ensure_unique_relay_url(state, &relay_url, Some(existing.relay_id)).await?;
    let tls_mode = body.tls_mode.unwrap_or(existing.tls_mode());
    let bridge_encryption_mode = body
        .bridge_encryption_mode
        .unwrap_or(existing.bridge_encryption_mode());
    let relay_ca = resolve_secret_patch(
        manager,
        body.relay_ca_pem,
        existing.relay_ca_envelope(),
        "relay_ca_pem",
    )?;
    let client_cert = resolve_secret_patch(
        manager,
        body.client_cert_pem,
        existing.client_cert_envelope(),
        "client_cert_pem",
    )?;
    let client_key = resolve_secret_patch(
        manager,
        body.client_key_pem,
        existing.client_key_envelope(),
        "client_key_pem",
    )?;
    let bridge_encryption_key = resolve_secret_patch(
        manager,
        body.bridge_encryption_key,
        existing.bridge_encryption_key_envelope(),
        "bridge_encryption_key",
    )?;
    validate_final_relay_config(
        &relay_url,
        tls_mode,
        bridge_encryption_mode,
        client_cert.is_some(),
        client_key.is_some(),
        bridge_encryption_key.is_some(),
        decrypt_secret(manager, relay_ca.as_ref())?,
        decrypt_secret(manager, client_cert.as_ref())?,
        decrypt_secret(manager, client_key.as_ref())?,
        decrypt_secret(manager, bridge_encryption_key.as_ref())?,
    )?;
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

fn decrypt_secret(
    manager: &crate::relay_secrets::RelaySecretManager,
    secret: Option<&crate::relay_secrets::EncryptedSecretEnvelope>,
) -> Result<Option<String>, Response> {
    secret
        .map(|value| manager.decrypt(value))
        .transpose()
        .map_err(|err| bad_request(&err.to_string()))
}
