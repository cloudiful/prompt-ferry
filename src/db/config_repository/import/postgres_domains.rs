//! Domain inserts for the PostgreSQL configuration import.
//!
//! Split from [`super::postgres`] to keep both files cohesive; every statement
//! runs on the transaction connection passed in, so it shares the single
//! replace transaction opened by [`super::postgres::apply`].

use anyhow::{Context, Result};
use sea_orm::{EntityTrait, Set};
use sqlx::{PgConnection, QueryBuilder};

use super::super::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY;
use super::super::snapshot::{
    McpCredentialSnapshot, McpServerSnapshot, SettingSnapshot, SnapshotDomains,
};
use super::postgres::run;
use crate::db::entities::postgres::model_route_targets;
use crate::db::seaorm::PostgresNativeConnection;
use crate::relay_secrets::RelaySecretManager;

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
    let connection = PostgresNativeConnection::new(&mut *conn);
    for (rule_id, stamp, target) in targets {
        model_route_targets::Entity::insert(model_route_targets::ActiveModel {
            target_id: Set(target.target_id),
            rule_id: Set(rule_id),
            endpoint_id: Set(target.endpoint_id),
            position: Set(target.position),
            enabled: Set(target.enabled),
            created_at: Set(stamp),
            updated_at: Set(stamp),
            upstream_model: Set(target.upstream_model.clone()),
            proxy_url_override: Set(target.proxy_url_override.clone()),
            active_windows: Set(target.active_windows.clone()),
            dev_system_normalize: Set(target.dev_system_normalize),
            native_api: Set(target.native_api.clone()),
            thinking_effort_override: Set(target.thinking_effort_override.clone()),
            compact_mode: Set(target.compact_mode.clone()),
            thinking_downgrade_enabled: Set(target.thinking_downgrade_enabled),
            service_tier: Set(target.service_tier.clone()),
            // The archive carries the dimension override, so the
            // restore keeps it.
            embedding_dimensions_override: Set(target.embedding_dimensions_override),
            // Manual new-session preference.
            prefer_new_sessions: Set(target.prefer_new_sessions),
            ..Default::default()
        })
        .exec(&connection)
        .await
        .map_err(crate::db::seaorm::native_error)?;
    }
    Ok(())
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
    // The four retired money columns still exist on this table until the
    // storage cutover, so they are left to their database defaults: the
    // importer neither reads nor writes any archived financial value.
    let mut query = QueryBuilder::new(
        "INSERT INTO mcp_credentials (credential_id, server_id, credential_label, secret, \
         position, enabled, provider_kind, created_at, updated_at) ",
    );
    query.push_values(&domains.mcp_credentials, |mut row, credential| {
        row.push_bind(credential.credential_id)
            .push_bind(credential.server_id)
            .push_bind(credential.credential_label.clone())
            .push_bind(credential.secret.clone())
            .push_bind(credential.position)
            .push_bind(credential.enabled)
            .push_bind(credential.provider_kind.clone())
            .push_bind(credential.created_at)
            .push_bind(credential.updated_at);
    });
    run(conn, query).await
}

/// Install the personal MCP OAuth grants of the archive.
///
/// The grants arrive decrypted and are sealed again with the destination
/// secret manager inside the caller's transaction, so the source ciphertext,
/// nonce and key version never travel and never survive the restore. The
/// helper installs a fresh connection epoch and the initial generation, and
/// deliberately restores no pending authorization state and no refresh lease.
pub(super) async fn insert_mcp_oauth_grants(
    conn: &mut PgConnection,
    manager: &RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    for grant in &domains.mcp_oauth {
        let payload = crate::db::McpOAuthGrant {
            registration: grant.registration.clone(),
            credentials: grant.credentials.clone(),
        };
        crate::db::mcp_oauth::install_grant_in_transaction(
            conn,
            manager,
            grant.server_id,
            grant.owner_user_id,
            &grant.config_fingerprint,
            &payload,
        )
        .await
        .context("failed to restore a personal MCP OAuth grant")?;
    }
    Ok(())
}

/// The settings rows an import restores.
///
/// The raw object-store configuration is deployment state and is never
/// imported, the shared Valkey coordination row belongs to the destination
/// deployment and keeps its own seed, and the internal coordination rows are
/// never restored.
pub(super) fn restorable_settings(domains: &SnapshotDomains) -> Vec<&SettingSnapshot> {
    domains
        .settings
        .iter()
        .filter(|setting| setting.key != LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY)
        .filter(|setting| setting.key != crate::db::SHARED_VALKEY_SETTINGS_KEY)
        .filter(|setting| {
            !crate::db::INTERNAL_COORDINATION_SETTING_KEYS.contains(&setting.key.as_str())
        })
        .collect()
}

pub(super) async fn insert_settings(
    conn: &mut PgConnection,
    _manager: &RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    let settings = restorable_settings(domains);
    if settings.is_empty() {
        return Ok(());
    }
    let mut query =
        QueryBuilder::new("INSERT INTO worker_settings (setting_key, setting_value, updated_at) ");
    query.push_values(settings, |mut row, setting| {
        row.push_bind(setting.key.clone())
            .push_bind(setting.value.clone())
            .push_bind(setting.updated_at.unwrap_or_else(chrono::Utc::now));
    });
    run(conn, query).await
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

/// Restore explicit per-user endpoint selections after their users and
/// endpoints exist. A NULL endpoint means "use the routing default" and is
/// inserted as NULL, distinct from the absence of the row.
pub(super) async fn insert_user_endpoint_settings(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    if domains.user_endpoint_settings.is_empty() {
        return Ok(());
    }
    let mut query =
        QueryBuilder::new("INSERT INTO user_endpoint_settings (user_id, endpoint_id, updated_at) ");
    query.push_values(&domains.user_endpoint_settings, |mut row, setting| {
        row.push_bind(setting.user_id)
            .push_bind(setting.endpoint_id)
            .push_bind(chrono::Utc::now());
    });
    run(conn, query).await
}

#[cfg(test)]
mod tests {
    use super::restorable_settings;
    use crate::db::config_repository::snapshot::{SettingSnapshot, SnapshotDomains};

    fn setting(key: &str) -> SettingSnapshot {
        SettingSnapshot {
            key: key.to_string(),
            version: 1,
            value: serde_json::Value::Null,
            updated_at: None,
        }
    }

    #[test]
    fn the_shared_valkey_row_is_never_restored() {
        let domains = SnapshotDomains {
            settings: vec![
                setting("redaction_enabled"),
                setting(crate::db::SHARED_VALKEY_SETTINGS_KEY),
                setting(crate::db::RELAY_SNAPSHOT_VERSION_SETTINGS_KEY),
                setting(crate::db::config_repository::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY),
            ],
            ..crate::db::config_repository::snapshot_source::empty_domains()
        };
        let restorable = restorable_settings(&domains);
        let keys: Vec<&str> = restorable
            .iter()
            .map(|setting| setting.key.as_str())
            .collect();
        assert_eq!(keys, vec!["redaction_enabled"]);
    }
}
