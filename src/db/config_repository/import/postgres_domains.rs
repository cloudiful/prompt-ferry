//! Domain inserts for the PostgreSQL configuration import.
//!
//! Split from [`super::postgres`] to keep both files cohesive; every statement
//! runs on the transaction connection passed in, so it shares the single
//! replace transaction opened by [`super::postgres::apply`].

use anyhow::{Context, Result};
use sqlx::{PgConnection, QueryBuilder};

use super::super::snapshot::{McpCredentialSnapshot, McpServerSnapshot, SnapshotDomains};
use super::postgres::run;
use crate::db::settings::RAW_OBJECT_STORE_SETTINGS_KEY;
use crate::raw_payload_store::{RawObjectStoreConfig, RawObjectStorePersisted};
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

pub(super) async fn insert_route_targets(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    let mut targets = Vec::new();
    for route in domains
        .model_routes
        .iter()
        .filter(|route| !route.targets.is_empty())
    {
        let stamp = route
            .updated_at
            .unwrap_or(route.created_at.unwrap_or_else(chrono::Utc::now));
        for target in &route.targets {
            targets.push((route.rule_id, stamp, target));
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO model_route_targets (target_id, rule_id, endpoint_id, position, enabled, \
         created_at, updated_at, upstream_model, proxy_url_override, active_windows, \
         dev_system_normalize, native_api, thinking_effort_override, compact_mode, \
         thinking_downgrade_enabled, service_tier) ",
    );
    query.push_values(targets, |mut row, (rule_id, stamp, target)| {
        row.push_bind(target.target_id)
            .push_bind(rule_id)
            .push_bind(target.endpoint_id)
            .push_bind(target.position)
            .push_bind(target.enabled)
            .push_bind(stamp)
            .push_bind(stamp)
            .push_bind(target.upstream_model.clone())
            .push_bind(target.proxy_url_override.clone())
            .push_bind(target.active_windows.clone())
            .push_bind(target.dev_system_normalize)
            .push_bind(target.native_api.clone())
            .push_bind(target.thinking_effort_override.clone())
            .push_bind(target.compact_mode.clone())
            .push_bind(target.thinking_downgrade_enabled)
            .push_bind(target.service_tier.clone());
    });
    run(conn, query).await
}

/// Rebuild `bearer_tokens_json` from the credential rows so the server keeps
/// its token positions and enabled flags after a round-trip.
fn bearer_tokens_json(server: &McpServerSnapshot, domains: &SnapshotDomains) -> serde_json::Value {
    let mut tokens: Vec<&McpCredentialSnapshot> = domains
        .mcp_credentials
        .iter()
        .filter(|credential| credential.server_id == server.server_id)
        .collect();
    tokens.sort_by_key(|credential| credential.position);
    serde_json::Value::Array(
        tokens
            .iter()
            .map(|credential| {
                serde_json::json!({ "token": credential.secret, "enabled": credential.enabled })
            })
            .collect(),
    )
}

pub(super) async fn insert_mcp_servers(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    if domains.mcp_servers.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO mcp_servers (server_id, scope, owner_user_id, source_endpoint_id, name, \
         aggregate_naming_mode, transport, provider_kind, url, command, args, env_json, \
         bearer_tokens_json, http_headers_json, auth_mode, basic_username, basic_password, \
         proxy_url, tool_filter_mode, allowed_tools, disabled_tools, disabled_resources, \
         enabled, timeout_ms, lifecycle_policy, lifecycle_manual_protocol_version, created_at, \
         updated_at) ",
    );
    query.push_values(&domains.mcp_servers, |mut row, server| {
        row.push_bind(server.server_id)
            .push_bind(server.scope.clone())
            .push_bind(server.owner_user_id)
            .push_bind(server.source_endpoint_id)
            .push_bind(server.name.clone())
            .push_bind(server.aggregate_naming_mode.clone())
            .push_bind(server.transport.clone())
            .push_bind(server.provider_kind.clone())
            .push_bind(server.url.clone())
            .push_bind(server.command.clone())
            .push_bind(server.args.clone())
            .push_bind(server.env_json.clone())
            .push_bind(bearer_tokens_json(server, domains))
            .push_bind(server.http_headers_json.clone())
            .push_bind(server.auth_mode.clone())
            .push_bind(server.basic_username.clone())
            .push_bind(server.basic_password.clone())
            .push_bind(server.proxy_url.clone())
            .push_bind(server.tool_filter_mode.clone())
            .push_bind(server.allowed_tools.clone())
            .push_bind(server.disabled_tools.clone())
            .push_bind(server.disabled_resources.clone())
            .push_bind(server.enabled)
            .push_bind(server.timeout_ms)
            .push_bind(server.lifecycle_policy.clone())
            .push_bind(server.lifecycle_manual_protocol_version.clone())
            .push_bind(server.created_at)
            .push_bind(server.updated_at);
    });
    run(conn, query).await
}

pub(super) async fn insert_mcp_credentials(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    if domains.mcp_credentials.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO mcp_credentials (credential_id, server_id, credential_label, secret, \
         position, enabled, provider_kind, default_cost, strict_mode, billing_period_start, \
         billing_period_end, created_at, updated_at) ",
    );
    query.push_values(&domains.mcp_credentials, |mut row, credential| {
        row.push_bind(credential.credential_id)
            .push_bind(credential.server_id)
            .push_bind(credential.credential_label.clone())
            .push_bind(credential.secret.clone())
            .push_bind(credential.position)
            .push_bind(credential.enabled)
            .push_bind(credential.provider_kind.clone())
            .push_bind(credential.default_cost)
            .push_bind(credential.strict_mode)
            .push_bind(credential.billing_period_start)
            .push_bind(credential.billing_period_end)
            .push_bind(credential.created_at)
            .push_bind(credential.updated_at);
    });
    run(conn, query).await
}

fn envelope(
    manager: &RelaySecretManager,
    value: Option<&String>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    match value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        Some(value) => Ok(Some(manager.encrypt(value)?)),
        None => Ok(None),
    }
}

pub(super) async fn insert_relays(
    conn: &mut PgConnection,
    manager: &RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    if domains.relays.is_empty() {
        return Ok(());
    }
    // Relay secrets are re-encrypted with the destination key before insert.
    let encrypted = domains
        .relays
        .iter()
        .map(|relay| {
            Ok((
                relay,
                envelope(manager, relay.relay_ca_pem.as_ref())?,
                envelope(manager, relay.client_cert_pem.as_ref())?,
                envelope(manager, relay.client_key_pem.as_ref())?,
                envelope(manager, relay.bridge_encryption_key.as_ref())?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut query = QueryBuilder::new(
        "INSERT INTO managed_relays (relay_id, name, relay_url, enabled, tls_mode, \
         bridge_encryption_mode, relay_ca_ciphertext, relay_ca_nonce, relay_ca_key_version, \
         client_cert_ciphertext, client_cert_nonce, client_cert_key_version, \
         client_key_ciphertext, client_key_nonce, client_key_key_version, \
         bridge_encryption_key_ciphertext, bridge_encryption_key_nonce, \
         bridge_encryption_key_key_version, created_at, updated_at) ",
    );
    query.push_values(encrypted, |mut row, (relay, ca, cert, key, bridge)| {
        let now = chrono::Utc::now();
        row.push_bind(relay.relay_id)
            .push_bind(relay.name.clone())
            .push_bind(relay.relay_url.clone())
            .push_bind(relay.enabled)
            .push_bind(relay.tls_mode.clone())
            .push_bind(relay.bridge_encryption_mode.clone())
            .push_bind(ca.as_ref().map(|e| e.ciphertext.clone()))
            .push_bind(ca.as_ref().map(|e| e.nonce.clone()))
            .push_bind(ca.as_ref().map(|e| e.key_version))
            .push_bind(cert.as_ref().map(|e| e.ciphertext.clone()))
            .push_bind(cert.as_ref().map(|e| e.nonce.clone()))
            .push_bind(cert.as_ref().map(|e| e.key_version))
            .push_bind(key.as_ref().map(|e| e.ciphertext.clone()))
            .push_bind(key.as_ref().map(|e| e.nonce.clone()))
            .push_bind(key.as_ref().map(|e| e.key_version))
            .push_bind(bridge.as_ref().map(|e| e.ciphertext.clone()))
            .push_bind(bridge.as_ref().map(|e| e.nonce.clone()))
            .push_bind(bridge.as_ref().map(|e| e.key_version))
            .push_bind(now)
            .push_bind(now);
    });
    run(conn, query).await
}

pub(super) async fn insert_settings(
    conn: &mut PgConnection,
    manager: &RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    let settings: Vec<&crate::db::config_repository::SettingSnapshot> = domains
        .settings
        .iter()
        .filter(|setting| setting.key != RAW_OBJECT_STORE_SETTINGS_KEY)
        .collect();
    if !settings.is_empty() {
        let mut query = QueryBuilder::new(
            "INSERT INTO worker_settings (setting_key, setting_value, updated_at) ",
        );
        query.push_values(settings, |mut row, setting| {
            row.push_bind(setting.key.clone())
                .push_bind(setting.value.clone())
                .push_bind(setting.updated_at.unwrap_or_else(chrono::Utc::now));
        });
        run(conn, query).await?;
    }
    if let Some(raw) = &domains.raw_object_store {
        let config: RawObjectStoreConfig = serde_json::from_value(raw.clone())
            .context("raw object store configuration is unreadable")?;
        let persisted = RawObjectStorePersisted::from_config(&config, manager)
            .context("failed to re-encrypt the raw object store configuration")?;
        let mut query =
            QueryBuilder::new("INSERT INTO worker_settings (setting_key, setting_value) ");
        query.push_values(
            [(RAW_OBJECT_STORE_SETTINGS_KEY, persisted)],
            |mut row, (key, value)| {
                row.push_bind(key)
                    .push_bind(serde_json::to_value(value).unwrap_or_default());
            },
        );
        run(conn, query).await?;
    }
    Ok(())
}

pub(super) async fn insert_user_redaction_configs(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    if domains.user_redaction_configs.is_empty() {
        return Ok(());
    }
    let mut query =
        QueryBuilder::new("INSERT INTO user_redaction_configs (user_id, config, updated_at) ");
    query.push_values(&domains.user_redaction_configs, |mut row, config| {
        row.push_bind(config.user_id)
            .push_bind(config.config.clone())
            .push_bind(chrono::Utc::now());
    });
    run(conn, query).await
}
