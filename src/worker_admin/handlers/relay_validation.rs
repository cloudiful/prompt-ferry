use super::*;
use crate::{
    config::{TlsMode, normalize_relay_url},
    tls,
};

pub(super) async fn ensure_unique_relay_url(
    state: &AdminState,
    relay_url: &str,
    current_relay_id: Option<Uuid>,
) -> Result<(), Box<Response>> {
    let relays = db::list_managed_relays(&state.pool)
        .await
        .map_err(|err| Box::new(internal(state, err)))?;
    if relays.into_iter().any(|relay| {
        Some(relay.relay_id) != current_relay_id
            && normalize_relay_url(&relay.relay_url) == relay_url
    }) {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "duplicate_relay_url",
            "relay_url must be unique after normalization",
        )));
    }
    Ok(())
}

pub(super) struct FinalRelayConfig<'a> {
    pub(super) relay_url: &'a str,
    pub(super) tls_mode: TlsMode,
    pub(super) bridge_encryption_mode: crate::config::BridgeEncryptionMode,
    pub(super) has_client_cert: bool,
    pub(super) has_client_key: bool,
    pub(super) has_bridge_key: bool,
    pub(super) relay_ca_pem: Option<String>,
    pub(super) client_cert_pem: Option<String>,
    pub(super) client_key_pem: Option<String>,
    pub(super) bridge_encryption_key: Option<String>,
}

pub(super) fn validate_final_relay_config(
    config: FinalRelayConfig<'_>,
) -> Result<(), Box<Response>> {
    tls::validate_worker_relay_material(
        config.relay_url,
        config.tls_mode,
        config.relay_ca_pem.as_deref(),
        config.client_cert_pem.as_deref(),
        config.client_key_pem.as_deref(),
    )
    .map_err(|err| Box::new(bad_request(&err.to_string())))?;
    if config.tls_mode == TlsMode::Mtls && (!config.has_client_cert || !config.has_client_key) {
        return Err(Box::new(bad_request(
            "client_cert_pem and client_key_pem are required when tls_mode=mtls",
        )));
    }
    if config.bridge_encryption_mode.required() && !config.has_bridge_key {
        return Err(Box::new(bad_request(
            "bridge_encryption_key is required when bridge_encryption_mode=required",
        )));
    }
    crate::bridge_crypto::validate_settings(
        "worker",
        config.bridge_encryption_mode,
        config.bridge_encryption_key.as_deref().unwrap_or_default(),
    )
    .map_err(|err| Box::new(bad_request(&err.to_string())))?;
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
