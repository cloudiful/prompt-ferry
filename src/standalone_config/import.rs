//! Envelope-aware SQLite replacement used by the administrator configuration
//! import.
//!
//! [`StandaloneConfigStore::replace_snapshot_for_import`] extends the existing
//! `replace_snapshot` transaction with the domains that live outside
//! [`StandaloneConfig`]: users, MCP servers (with their reconstructed bearer
//! tokens), per-endpoint OAuth tokens, and the OpenAI Admin API key. Every
//! write shares one SQLite transaction, so a validation or persistence failure
//! rolls the whole import back and never leaves a half-restored instance.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use super::StandaloneConfigStore;
use super::write::{self, EncryptedConfig};
use crate::db::McpServerInput;
use crate::relay_secrets::RelaySecretManager;

/// One `standalone_users` row restored from the archive. `password_hash` is
/// the Argon2id PHC string copied verbatim; plaintext passwords never exist.
pub struct ImportUser {
    pub user_id: i64,
    pub login_name: String,
    pub display_name: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One MCP server with its reconstructed `bearer_tokens_json`.
pub struct ImportMcpServer {
    pub server_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub input: McpServerInput,
}

/// One per-endpoint ChatGPT OAuth token.
pub struct ImportEndpointOAuth {
    pub endpoint_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// One per-endpoint OpenAI Admin API key.
pub struct ImportEndpointAdminKey {
    pub endpoint_id: Uuid,
    pub api_key: String,
}

impl StandaloneConfigStore {
    /// Atomically replace the standalone configuration with the imported
    /// snapshot. The caller supplies already-decoded domain records; every
    /// secret is re-encrypted through `manager` inside the transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn replace_snapshot_for_import(
        &self,
        manager: &RelaySecretManager,
        snapshot: &super::StandaloneConfig,
        users: &[ImportUser],
        mcp_servers: &[ImportMcpServer],
        oauth_tokens: &[ImportEndpointOAuth],
        admin_keys: &[ImportEndpointAdminKey],
    ) -> Result<()> {
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, snapshot)?;
        let mut tx = self.pool().begin().await?;
        // `delete_endpoints` only removes endpoints absent from the temp id
        // set, so the archive's identifiers are booked first; this keeps
        // endpoint identity (and its usage references) stable across imports.
        standalone_query!("src/sql/standalone/create_snapshot_endpoint_ids.sql")
            .execute(&mut *tx)
            .await?;
        standalone_query!("src/sql/standalone/clear_snapshot_endpoint_ids.sql")
            .execute(&mut *tx)
            .await?;
        for endpoint in &snapshot.endpoints {
            standalone_query!("src/sql/standalone/save_snapshot_endpoint_id.sql")
                .bind(endpoint.endpoint_id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        standalone_query!("src/sql/standalone/delete_mcp_servers_all.sql")
            .execute(&mut *tx)
            .await?;
        write::delete_all(&mut tx).await?;
        standalone_query!("src/standalone_config/sql/users/delete_all_users.sql")
            .execute(&mut *tx)
            .await?;
        for user in users {
            insert_user(&mut tx, user).await?;
        }
        write::insert_all(&mut tx, &encrypted).await?;
        for server in mcp_servers {
            insert_mcp_server(&mut tx, manager, server).await?;
        }
        for token in oauth_tokens {
            insert_endpoint_oauth(&mut tx, manager, token).await?;
        }
        for key in admin_keys {
            insert_endpoint_admin_key(&mut tx, manager, key).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn insert_user(tx: &mut Transaction<'_, Sqlite>, user: &ImportUser) -> Result<()> {
    standalone_query!("src/standalone_config/sql/users/insert_user_with_id.sql")
        .bind(user.user_id)
        .bind(&user.login_name)
        .bind(&user.password_hash)
        .bind(&user.display_name)
        .bind(i64::from(user.is_admin))
        .bind(i64::from(user.is_active))
        .bind(user.created_at.to_rfc3339())
        .bind(user.updated_at.to_rfc3339())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_mcp_server(
    tx: &mut Transaction<'_, Sqlite>,
    manager: &RelaySecretManager,
    server: &ImportMcpServer,
) -> Result<()> {
    let input = &server.input;
    let env = manager.encrypt(&serde_json::to_string(&input.env_json)?)?;
    let bearer_tokens = manager.encrypt(&serde_json::to_string(&input.bearer_tokens_json)?)?;
    let basic_password = match input.basic_password.as_deref().map(str::trim) {
        Some(password) if !password.is_empty() => Some(manager.encrypt(password)?),
        _ => None,
    };
    let proxy = match input.proxy_url.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => Some(manager.encrypt(raw)?),
        _ => None,
    };
    standalone_query!("src/sql/standalone/save_mcp_server.sql")
        .bind(server.server_id.to_string())
        .bind(input.source_endpoint_id.map(|id| id.to_string()))
        .bind(&input.scope)
        .bind(input.owner_user_id)
        .bind(&input.name)
        .bind(&input.aggregate_naming_mode)
        .bind(&input.transport)
        .bind(
            input
                .provider_kind
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        )
        .bind(&input.url)
        .bind(&input.command)
        .bind(serde_json::to_string(&input.args)?)
        .bind(serde_json::to_string(&input.http_headers_json)?)
        .bind(&input.auth_mode)
        .bind(&input.basic_username)
        .bind(basic_password.as_ref().map(|e| e.ciphertext.clone()))
        .bind(basic_password.as_ref().map(|e| e.nonce.clone()))
        .bind(basic_password.as_ref().map(|e| i64::from(e.key_version)))
        .bind(&input.tool_filter_mode)
        .bind(serde_json::to_string(&input.allowed_tools)?)
        .bind(serde_json::to_string(&input.disabled_tools)?)
        .bind(serde_json::to_string(&input.disabled_resources)?)
        .bind(i64::from(input.enabled))
        .bind(input.timeout_ms)
        .bind(&input.lifecycle_policy)
        .bind(&input.lifecycle_manual_protocol_version)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(env.ciphertext)
        .bind(env.nonce)
        .bind(i64::from(env.key_version))
        .bind(bearer_tokens.ciphertext)
        .bind(bearer_tokens.nonce)
        .bind(i64::from(bearer_tokens.key_version))
        .bind(proxy.as_ref().map(|e| e.ciphertext.clone()))
        .bind(proxy.as_ref().map(|e| e.nonce.clone()))
        .bind(proxy.as_ref().map(|e| i64::from(e.key_version)))
        .bind(server.created_at.to_rfc3339())
        .bind(server.updated_at.to_rfc3339())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_endpoint_oauth(
    tx: &mut Transaction<'_, Sqlite>,
    manager: &RelaySecretManager,
    token: &ImportEndpointOAuth,
) -> Result<()> {
    let access = manager.encrypt(&token.access_token)?;
    let refresh = manager.encrypt(&token.refresh_token)?;
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    standalone_query!("src/sql/standalone/set_endpoint_oauth_token.sql")
        .bind(token.endpoint_id.to_string())
        .bind(Some(access.ciphertext))
        .bind(Some(access.nonce))
        .bind(Some(i64::from(access.key_version)))
        .bind(Some(refresh.ciphertext))
        .bind(Some(refresh.nonce))
        .bind(Some(i64::from(refresh.key_version)))
        .bind(
            token
                .expires_at
                .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)),
        )
        .bind(now.clone())
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_endpoint_admin_key(
    tx: &mut Transaction<'_, Sqlite>,
    manager: &RelaySecretManager,
    key: &ImportEndpointAdminKey,
) -> Result<()> {
    let envelope = manager.encrypt(key.api_key.trim())?;
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    standalone_query!("src/sql/standalone/set_endpoint_admin_key.sql")
        .bind(key.endpoint_id.to_string())
        .bind(Some(envelope.ciphertext))
        .bind(Some(envelope.nonce))
        .bind(Some(i64::from(envelope.key_version)))
        .bind(now.clone())
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
