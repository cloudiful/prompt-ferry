use sqlx::Row;

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::{Result, StandaloneConfigError};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn list_mcp_servers(
        &self,
        manager: &RelaySecretManager,
    ) -> Result<Vec<crate::db::McpServer>> {
        let rows = standalone_query!("src/sql/standalone/list_mcp_servers.sql")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| self.mcp_server_from_row(manager, &row))
            .collect()
    }

    pub async fn list_mcp_servers_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<crate::db::McpServer>)> {
        let total = standalone_query!("src/sql/standalone/count_mcp_servers.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_mcp_servers_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let servers = rows
            .iter()
            .map(|row| self.mcp_server_from_row(manager, row))
            .collect::<Result<Vec<_>>>()?;
        Ok((total, servers))
    }

    pub async fn list_user_mcp_servers_page(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<crate::db::McpServer>)> {
        let total = standalone_query!("src/sql/standalone/count_user_mcp_servers.sql")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_user_mcp_servers_page.sql")
            .bind(user_id)
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let servers = rows
            .iter()
            .map(|row| self.mcp_server_from_row(manager, row))
            .collect::<Result<Vec<_>>>()?;
        Ok((total, servers))
    }

    pub async fn list_visible_mcp_servers(
        &self,
        manager: &RelaySecretManager,
        user_id: Option<i64>,
    ) -> Result<Vec<crate::db::McpServer>> {
        let rows = standalone_query!("src/sql/standalone/list_visible_mcp_servers.sql")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| self.mcp_server_from_row(manager, &row))
            .collect()
    }

    pub async fn get_mcp_server(
        &self,
        manager: &RelaySecretManager,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server.sql")
            .bind(server_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_user_mcp_server(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_user_mcp_server.sql")
            .bind(server_id.to_string())
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_mcp_server_by_name(
        &self,
        manager: &RelaySecretManager,
        name: &str,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server_by_name.sql")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_mcp_server_by_source_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server_by_source_endpoint.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn save_mcp_server(
        &self,
        manager: &RelaySecretManager,
        server_id: uuid::Uuid,
        input: &crate::db::McpServerInput,
        existing: Option<&crate::db::McpServer>,
    ) -> Result<crate::db::McpServer> {
        let env = manager.encrypt(&serde_json::to_string(&input.env_json)?)?;
        let bearer_tokens = manager.encrypt(&serde_json::to_string(&input.bearer_tokens_json)?)?;
        let basic_password_envelope = match &input.basic_password {
            Some(password) if !password.trim().is_empty() => Some(manager.encrypt(password)?),
            _ => None,
        };
        // Issue #375 Phase F: per-row proxy envelope (0019). Empty means
        // inherit (NULL) so clears round-trip as NULL.
        let proxy_envelope = match input.proxy_url.as_deref().map(str::trim) {
            Some(raw) if !raw.is_empty() => Some(manager.encrypt(raw)?),
            _ => None,
        };
        let now = chrono::Utc::now();
        standalone_query!("src/sql/standalone/save_mcp_server.sql")
            .bind(server_id.to_string())
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
            .bind(
                basic_password_envelope
                    .as_ref()
                    .map(|e| e.ciphertext.clone()),
            )
            .bind(basic_password_envelope.as_ref().map(|e| e.nonce.clone()))
            .bind(
                basic_password_envelope
                    .as_ref()
                    .map(|e| i64::from(e.key_version)),
            )
            .bind(&input.tool_filter_mode)
            .bind(serde_json::to_string(&input.allowed_tools)?)
            .bind(serde_json::to_string(&input.disabled_tools)?)
            .bind(serde_json::to_string(&input.disabled_resources)?)
            .bind(i64::from(input.enabled))
            .bind(input.timeout_ms)
            .bind(&input.lifecycle_policy)
            .bind(&input.lifecycle_manual_protocol_version)
            .bind(existing.and_then(|server| server.lifecycle_learned_mode.as_deref()))
            .bind(existing.and_then(|server| server.lifecycle_learned_protocol_version.as_deref()))
            .bind(
                existing
                    .and_then(|server| server.lifecycle_learned_for_updated_at)
                    .map(|value| value.to_rfc3339()),
            )
            .bind(
                existing
                    .and_then(|server| server.lifecycle_learned_at)
                    .map(|value| value.to_rfc3339()),
            )
            .bind(env.ciphertext)
            .bind(env.nonce)
            .bind(i64::from(env.key_version))
            .bind(bearer_tokens.ciphertext)
            .bind(bearer_tokens.nonce)
            .bind(i64::from(bearer_tokens.key_version))
            .bind(proxy_envelope.as_ref().map(|e| e.ciphertext.clone()))
            .bind(proxy_envelope.as_ref().map(|e| e.nonce.clone()))
            .bind(proxy_envelope.as_ref().map(|e| i64::from(e.key_version)))
            .bind(
                existing
                    .map(|server| server.created_at)
                    .unwrap_or(now)
                    .to_rfc3339(),
            )
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
        self.get_mcp_server(manager, server_id)
            .await?
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase("MCP server missing after save".to_string())
            })
    }

    pub async fn delete_mcp_server(&self, server_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_mcp_server.sql")
            .bind(server_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_mcp_lifecycle_learned(
        &self,
        server: &crate::db::McpServer,
        mode: &str,
        protocol_version: &str,
    ) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/mark_mcp_lifecycle_learned.sql")
            .bind(mode)
            .bind(protocol_version)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(server.server_id.to_string())
            .bind(server.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    fn mcp_server_from_row(
        &self,
        manager: &RelaySecretManager,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<crate::db::McpServer> {
        let (mut server, env, bearer_tokens, basic_password_envelope, proxy_envelope) =
            rows::mcp_server(row)?;
        server.env_json = decrypt_json(manager, &env, "MCP environment")?;
        server.bearer_tokens_json = decrypt_json(manager, &bearer_tokens, "MCP bearer tokens")?;
        server.basic_password = match basic_password_envelope {
            Some(envelope) => Some(manager.decrypt(&envelope)?),
            None => None,
        };
        // Issue #375 Phase F: decrypt the 0019 proxy envelope; presence
        // means saved (drives `has_proxy_url`).
        server.proxy_url =
            crate::standalone_config::write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
        server.has_proxy_url = server
            .proxy_url
            .as_deref()
            .is_some_and(|raw| !raw.trim().is_empty());
        Ok(server)
    }
}

fn decrypt_json(
    manager: &RelaySecretManager,
    envelope: &crate::relay_secrets::EncryptedSecretEnvelope,
    label: &str,
) -> Result<serde_json::Value> {
    let value = manager.decrypt(envelope)?;
    serde_json::from_str(&value).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!("{label} JSON is invalid: {error}"))
    })
}
