use sea_orm::sea_query::{Expr, ExprTrait, OnConflict, Order};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};

use crate::db::entities::sqlite::standalone_mcp_servers::{ActiveModel, Column, Entity};
use crate::db::seaorm::sqlite_connection;
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::{Result, StandaloneConfigError};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn list_mcp_servers(
        &self,
        manager: &RelaySecretManager,
    ) -> Result<Vec<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let models = Entity::find()
            .order_by_desc(Column::Scope)
            .order_by(Expr::col(Column::OwnerUserId).is_not_null(), Order::Asc)
            .order_by_asc(Column::OwnerUserId)
            .order_by_asc(Column::Name)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        models
            .iter()
            .map(|model| self.mcp_server_from_model(manager, model))
            .collect()
    }

    pub async fn list_mcp_servers_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<crate::db::McpServer>)> {
        let db = sqlite_connection(&self.pool);
        let total = Entity::find()
            .count(&db)
            .await
            .map_err(rows::native_error)? as i64;
        let models = Entity::find()
            .order_by_desc(Column::Scope)
            .order_by(Expr::col(Column::OwnerUserId).is_not_null(), Order::Asc)
            .order_by_asc(Column::OwnerUserId)
            .order_by_asc(Column::Name)
            .limit(rows.clamp(1, 200) as u64)
            .offset(std::cmp::Ord::max(first, 0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let servers = models
            .iter()
            .map(|model| self.mcp_server_from_model(manager, model))
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
        let db = sqlite_connection(&self.pool);
        let total = Entity::find()
            .filter(Column::Scope.eq("user"))
            .filter(Column::OwnerUserId.eq(user_id))
            .count(&db)
            .await
            .map_err(rows::native_error)? as i64;
        let models = Entity::find()
            .filter(Column::Scope.eq("user"))
            .filter(Column::OwnerUserId.eq(user_id))
            .order_by_asc(Column::Name)
            .limit(rows.clamp(1, 200) as u64)
            .offset(std::cmp::Ord::max(first, 0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let servers = models
            .iter()
            .map(|model| self.mcp_server_from_model(manager, model))
            .collect::<Result<Vec<_>>>()?;
        Ok((total, servers))
    }

    pub async fn list_visible_mcp_servers(
        &self,
        manager: &RelaySecretManager,
        user_id: Option<i64>,
    ) -> Result<Vec<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let models = Entity::find()
            .filter(Column::Enabled.eq(1))
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(Column::Scope.eq("admin"))
                            .add(Column::OwnerUserId.is_null()),
                    )
                    .add(
                        Condition::all()
                            .add(Column::Scope.eq("user"))
                            .add(Column::OwnerUserId.eq(user_id)),
                    ),
            )
            .order_by_asc(Column::Name)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        models
            .iter()
            .map(|model| self.mcp_server_from_model(manager, model))
            .collect()
    }

    pub async fn get_mcp_server(
        &self,
        manager: &RelaySecretManager,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find_by_id(server_id.to_string())
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        model
            .as_ref()
            .map(|model| self.mcp_server_from_model(manager, model))
            .transpose()
    }

    pub async fn get_user_mcp_server(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find_by_id(server_id.to_string())
            .filter(Column::Scope.eq("user"))
            .filter(Column::OwnerUserId.eq(user_id))
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        model
            .as_ref()
            .map(|model| self.mcp_server_from_model(manager, model))
            .transpose()
    }

    pub async fn get_mcp_server_by_name(
        &self,
        manager: &RelaySecretManager,
        name: &str,
    ) -> Result<Option<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find()
            .filter(Column::Name.eq(name))
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        model
            .as_ref()
            .map(|model| self.mcp_server_from_model(manager, model))
            .transpose()
    }

    pub async fn get_mcp_server_by_source_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find()
            .filter(Column::SourceEndpointId.eq(endpoint_id.to_string()))
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        model
            .as_ref()
            .map(|model| self.mcp_server_from_model(manager, model))
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
        let db = sqlite_connection(&self.pool);
        Entity::insert(ActiveModel {
            server_id: Set(server_id.to_string()),
            source_endpoint_id: Set(input.source_endpoint_id.map(|id| id.to_string())),
            scope: Set(input.scope.clone()),
            owner_user_id: Set(input.owner_user_id),
            name: Set(input.name.clone()),
            aggregate_naming_mode: Set(input.aggregate_naming_mode.clone()),
            transport: Set(input.transport.clone()),
            provider_kind: Set(input
                .provider_kind
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)),
            url: Set(input.url.clone()),
            command: Set(input.command.clone()),
            args_json: Set(serde_json::to_string(&input.args)?),
            http_headers_json: Set(serde_json::to_string(&input.http_headers_json)?),
            auth_mode: Set(input.auth_mode.clone()),
            basic_username: Set(input.basic_username.clone()),
            basic_password_ciphertext: Set(basic_password_envelope
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            basic_password_nonce: Set(basic_password_envelope
                .as_ref()
                .map(|envelope| envelope.nonce.clone())),
            basic_password_key_version: Set(basic_password_envelope
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            tool_filter_mode: Set(input.tool_filter_mode.clone()),
            allowed_tools_json: Set(serde_json::to_string(&input.allowed_tools)?),
            disabled_tools_json: Set(serde_json::to_string(&input.disabled_tools)?),
            disabled_resources_json: Set(serde_json::to_string(&input.disabled_resources)?),
            enabled: Set(i32::from(input.enabled)),
            timeout_ms: Set(input.timeout_ms),
            lifecycle_policy: Set(input.lifecycle_policy.clone()),
            lifecycle_manual_protocol_version: Set(input.lifecycle_manual_protocol_version.clone()),
            lifecycle_learned_mode: Set(
                existing.and_then(|server| server.lifecycle_learned_mode.clone())
            ),
            lifecycle_learned_protocol_version: Set(
                existing.and_then(|server| server.lifecycle_learned_protocol_version.clone())
            ),
            lifecycle_learned_for_updated_at: Set(existing
                .and_then(|server| server.lifecycle_learned_for_updated_at)
                .map(|value| value.to_rfc3339())),
            lifecycle_learned_at: Set(existing
                .and_then(|server| server.lifecycle_learned_at)
                .map(|value| value.to_rfc3339())),
            env_ciphertext: Set(env.ciphertext),
            env_nonce: Set(env.nonce),
            env_key_version: Set(i32::from(env.key_version)),
            bearer_tokens_ciphertext: Set(bearer_tokens.ciphertext),
            bearer_tokens_nonce: Set(bearer_tokens.nonce),
            bearer_tokens_key_version: Set(i32::from(bearer_tokens.key_version)),
            proxy_url_ciphertext: Set(proxy_envelope
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            proxy_url_nonce: Set(proxy_envelope
                .as_ref()
                .map(|envelope| envelope.nonce.clone())),
            proxy_url_key_version: Set(proxy_envelope
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            created_at: Set(existing
                .map(|server| server.created_at)
                .unwrap_or(now)
                .to_rfc3339()),
            updated_at: Set(now.to_rfc3339()),
        })
        .on_conflict(
            OnConflict::column(Column::ServerId)
                .update_columns([
                    Column::SourceEndpointId,
                    Column::Scope,
                    Column::OwnerUserId,
                    Column::Name,
                    Column::AggregateNamingMode,
                    Column::Transport,
                    Column::ProviderKind,
                    Column::Url,
                    Column::Command,
                    Column::ArgsJson,
                    Column::HttpHeadersJson,
                    Column::AuthMode,
                    Column::BasicUsername,
                    Column::BasicPasswordCiphertext,
                    Column::BasicPasswordNonce,
                    Column::BasicPasswordKeyVersion,
                    Column::ToolFilterMode,
                    Column::AllowedToolsJson,
                    Column::DisabledToolsJson,
                    Column::DisabledResourcesJson,
                    Column::Enabled,
                    Column::TimeoutMs,
                    Column::LifecyclePolicy,
                    Column::LifecycleManualProtocolVersion,
                    Column::LifecycleLearnedMode,
                    Column::LifecycleLearnedProtocolVersion,
                    Column::LifecycleLearnedForUpdatedAt,
                    Column::LifecycleLearnedAt,
                    Column::EnvCiphertext,
                    Column::EnvNonce,
                    Column::EnvKeyVersion,
                    Column::BearerTokensCiphertext,
                    Column::BearerTokensNonce,
                    Column::BearerTokensKeyVersion,
                    Column::ProxyUrlCiphertext,
                    Column::ProxyUrlNonce,
                    Column::ProxyUrlKeyVersion,
                    Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(&db)
        .await
        .map_err(rows::native_error)?;
        self.get_mcp_server(manager, server_id)
            .await?
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase("MCP server missing after save".to_string())
            })
    }

    pub async fn delete_mcp_server(&self, server_id: uuid::Uuid) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::delete_many()
            .filter(Column::ServerId.eq(server_id.to_string()))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(result.rows_affected > 0)
    }

    pub async fn mark_mcp_lifecycle_learned(
        &self,
        server: &crate::db::McpServer,
        mode: &str,
        protocol_version: &str,
    ) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::update_many()
            .col_expr(Column::LifecycleLearnedMode, Expr::value(mode))
            .col_expr(
                Column::LifecycleLearnedProtocolVersion,
                Expr::value(protocol_version),
            )
            .col_expr(
                Column::LifecycleLearnedForUpdatedAt,
                Expr::col(Column::UpdatedAt),
            )
            .col_expr(
                Column::LifecycleLearnedAt,
                Expr::value(chrono::Utc::now().to_rfc3339()),
            )
            .filter(Column::ServerId.eq(server.server_id.to_string()))
            .filter(Column::UpdatedAt.eq(server.updated_at.to_rfc3339()))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(result.rows_affected > 0)
    }

    fn mcp_server_from_model(
        &self,
        manager: &RelaySecretManager,
        model: &crate::db::entities::sqlite::standalone_mcp_servers::Model,
    ) -> Result<crate::db::McpServer> {
        let (mut server, env, bearer_tokens, basic_password_envelope, proxy_envelope) =
            rows::mcp_server_from_model(model)?;
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
