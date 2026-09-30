//! SQLite (standalone) configuration import.
//!
//! Converts the portable snapshot domains into the standalone store's typed
//! configuration and hands the whole replacement to
//! [`StandaloneConfigStore::replace_snapshot_for_import`], which keeps every
//! write in one envelope-aware SQLite transaction. The row conversions that
//! live outside the [`StandaloneConfig`] shape (users, MCP servers, per-endpoint
//! OAuth and admin keys) live in [`super::sqlite_rows`].

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use std::sync::Arc;

use super::super::snapshot::{
    EndpointApiKeySnapshot, EndpointSnapshot, ModelRouteSnapshot, ModelRouteTargetSnapshot,
    SecretRecovery, SnapshotDomains,
};
use super::ImportError;
use super::sqlite_rows;
use crate::standalone_config::{
    ClientKeyConfig, EndpointApiKeyConfig, ManagedRelayConfig, ModelRouteConfig,
    ModelRouteTargetConfig, ProviderEndpointConfig, SettingConfig, StandaloneConfig,
    StandaloneConfigStore,
};

/// Parse a snapshot string into its typed enum. The archive stores the enum's
/// own serialized form, so a value that fails here is a foreign payload.
fn parse_enum<T: DeserializeOwned>(value: &str, what: &'static str) -> Result<T, ImportError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|_| ImportError::InvalidArchive(what))
}

pub(super) async fn apply(
    store: &Arc<StandaloneConfigStore>,
    manager: &crate::relay_secrets::RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    let config = build_config(domains)?;
    let users = sqlite_rows::build_users(domains)?;
    let mcp_servers = sqlite_rows::build_mcp_servers(domains)?;
    let oauth_tokens = sqlite_rows::build_oauth_tokens(domains)?;
    let admin_keys = sqlite_rows::build_admin_keys(domains)?;
    store
        .replace_snapshot_for_import(
            manager,
            &config,
            &users,
            &mcp_servers,
            &oauth_tokens,
            &admin_keys,
        )
        .await
}

fn build_config(domains: &SnapshotDomains) -> Result<StandaloneConfig, ImportError> {
    Ok(StandaloneConfig {
        relays: domains
            .relays
            .iter()
            .map(|relay| {
                Ok(ManagedRelayConfig {
                    relay_id: relay.relay_id,
                    name: relay.name.clone(),
                    relay_url: relay.relay_url.clone(),
                    enabled: relay.enabled,
                    tls_mode: parse_enum(&relay.tls_mode, "relay TLS mode")?,
                    bridge_encryption_mode: parse_enum(
                        &relay.bridge_encryption_mode,
                        "bridge encryption mode",
                    )?,
                    relay_ca_pem: relay.relay_ca_pem.clone(),
                    client_cert_pem: relay.client_cert_pem.clone(),
                    client_key_pem: relay.client_key_pem.clone(),
                    bridge_encryption_key: relay.bridge_encryption_key.clone(),
                })
            })
            .collect::<Result<Vec<_>, ImportError>>()?,
        endpoints: domains
            .endpoints
            .iter()
            .map(convert_endpoint)
            .collect::<Result<Vec<_>, ImportError>>()?,
        routes: domains
            .model_routes
            .iter()
            .map(convert_route)
            .collect::<Result<Vec<_>, ImportError>>()?,
        client_keys: domains
            .client_keys
            .iter()
            .map(convert_client_key)
            .collect::<Result<Vec<_>, ImportError>>()?,
        settings: domains
            .settings
            .iter()
            .map(|setting| SettingConfig {
                key: setting.key.clone(),
                version: setting.version,
                value: setting.value.clone(),
            })
            .collect(),
    })
}

fn convert_client_key(
    key: &super::super::snapshot::ClientKeySnapshot,
) -> Result<ClientKeyConfig, ImportError> {
    if key.secret_state != SecretRecovery::Recovered {
        return Err(ImportError::InvalidArchive(
            "a client key has no recoverable secret for this backend",
        ));
    }
    let secret = key.secret.clone().ok_or(ImportError::InvalidArchive(
        "a client key has no recoverable secret for this backend",
    ))?;
    Ok(ClientKeyConfig {
        key_id: key.key_id,
        user_id: key.user_id,
        key_prefix: key.key_prefix.clone(),
        label: key.label.clone(),
        secret,
        enabled: key.enabled,
    })
}

fn convert_endpoint(endpoint: &EndpointSnapshot) -> Result<ProviderEndpointConfig, ImportError> {
    let fallback_timestamp = endpoint.updated_at;
    let api_keys = endpoint
        .api_keys
        .iter()
        .map(|key| convert_api_key(key, endpoint.endpoint_id, fallback_timestamp))
        .collect::<Result<Vec<_>, ImportError>>()?;
    let api_key = endpoint
        .api_key
        .clone()
        .or_else(|| {
            endpoint
                .api_keys
                .first()
                .and_then(|key| key.api_key.clone())
        })
        .unwrap_or_default();
    Ok(ProviderEndpointConfig {
        endpoint_id: endpoint.endpoint_id,
        name: endpoint.name.clone(),
        provider: parse_enum(&endpoint.provider, "endpoint provider")?,
        provider_region: endpoint
            .provider_region
            .as_deref()
            .map(|value| parse_enum(value, "endpoint region"))
            .transpose()?,
        service_tier: endpoint.service_tier.clone(),
        base_url: endpoint.base_url.clone(),
        native_api: parse_enum(&endpoint.native_api, "endpoint native API")?,
        native_api_source: parse_enum(&endpoint.native_api_source, "endpoint native API source")?,
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        api_key,
        api_keys,
        proxy_url: endpoint.proxy_url.clone(),
        active_windows: endpoint.active_windows.clone(),
    })
}

fn convert_api_key(
    key: &EndpointApiKeySnapshot,
    endpoint_id: uuid::Uuid,
    timestamp: DateTime<Utc>,
) -> Result<EndpointApiKeyConfig, ImportError> {
    let api_key = key.api_key.clone().ok_or(ImportError::InvalidArchive(
        "an endpoint API key has no recoverable value",
    ))?;
    Ok(EndpointApiKeyConfig {
        key_id: key.key_id,
        endpoint_id,
        key_label: key.key_label.clone(),
        api_key,
        position: key.position,
        enabled: key.enabled,
        created_at: timestamp,
        updated_at: timestamp,
    })
}

fn convert_route(route: &ModelRouteSnapshot) -> Result<ModelRouteConfig, ImportError> {
    Ok(ModelRouteConfig {
        rule_id: route.rule_id,
        scope: parse_enum(&route.scope, "route scope")?,
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern.clone(),
        routing_strategy: parse_enum(&route.routing_strategy, "routing strategy")?,
        enabled: route.enabled,
        targets: route
            .targets
            .iter()
            .map(convert_target)
            .collect::<Result<Vec<_>, ImportError>>()?,
    })
}

fn convert_target(
    target: &ModelRouteTargetSnapshot,
) -> Result<ModelRouteTargetConfig, ImportError> {
    Ok(ModelRouteTargetConfig {
        target_id: target.target_id,
        endpoint_id: target.endpoint_id,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model.clone(),
        native_api: parse_enum(&target.native_api, "target native API")?,
        proxy_url_override: target.proxy_url_override.clone(),
        active_windows: target.active_windows.clone(),
        dev_system_normalize: target.dev_system_normalize,
        thinking_downgrade_enabled: target.thinking_downgrade_enabled,
        thinking_effort_override: target.thinking_effort_override.clone(),
        compact_mode: target.compact_mode.clone(),
        service_tier: target.service_tier.clone(),
    })
}
