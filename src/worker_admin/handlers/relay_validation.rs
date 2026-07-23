use super::*;
use crate::{
    config::{TlsMode, normalize_relay_url},
    tls,
};

pub(super) async fn ensure_unique_relay_url(
    state: &AdminState,
    relay_url: &str,
    current_relay_id: Option<Uuid>,
) -> Result<(), Response> {
    let relays = db::list_managed_relays(&state.pool)
        .await
        .map_err(|err| internal(state, err))?;
    if relays.into_iter().any(|relay| {
        Some(relay.relay_id) != current_relay_id
            && normalize_relay_url(&relay.relay_url) == relay_url
    }) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "duplicate_relay_url",
            "relay_url must be unique after normalization",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_final_relay_config(
    relay_url: &str,
    tls_mode: TlsMode,
    bridge_encryption_mode: crate::config::BridgeEncryptionMode,
    has_client_cert: bool,
    has_client_key: bool,
    has_bridge_key: bool,
    relay_ca_pem: Option<String>,
    client_cert_pem: Option<String>,
    client_key_pem: Option<String>,
    bridge_encryption_key: Option<String>,
) -> Result<(), Response> {
    tls::validate_worker_relay_material(
        relay_url,
        tls_mode,
        relay_ca_pem.as_deref(),
        client_cert_pem.as_deref(),
        client_key_pem.as_deref(),
    )
    .map_err(|err| bad_request(&err.to_string()))?;
    if tls_mode == TlsMode::Mtls && (!has_client_cert || !has_client_key) {
        return Err(bad_request(
            "client_cert_pem and client_key_pem are required when tls_mode=mtls",
        ));
    }
    if bridge_encryption_mode.required() && !has_bridge_key {
        return Err(bad_request(
            "bridge_encryption_key is required when bridge_encryption_mode=required",
        ));
    }
    crate::bridge_crypto::validate_settings(
        "worker",
        bridge_encryption_mode,
        bridge_encryption_key.as_deref().unwrap_or_default(),
    )
    .map_err(|err| bad_request(&err.to_string()))?;
    Ok(())
}

pub(super) fn map_relay_db_error(state: &AdminState, err: anyhow::Error) -> Response {
    if let Some(db_err) = err.downcast_ref::<sqlx::Error>()
        && let sqlx::Error::Database(db_err) = db_err
        && db_err.constraint() == Some("managed_relays_relay_url_unique")
    {
        return error(
            StatusCode::BAD_REQUEST,
            "duplicate_relay_url",
            "relay_url must be unique",
        );
    }
    internal(state, err)
}
