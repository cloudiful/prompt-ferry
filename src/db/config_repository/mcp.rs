//! MCP configuration dispatch for PostgreSQL and encrypted standalone SQLite.

use anyhow::Result;
use uuid::Uuid;

use crate::db::{McpServer, McpServerInput, ProviderEndpoint};

impl super::ConfigRepository {
    pub async fn list_all_mcp_servers(&self) -> Result<Vec<McpServer>> {
        match self {
            Self::Postgres(repo) => crate::db::list_mcp_servers(repo.pool()).await,
            Self::Sqlite(repo) => repo
                .store()
                .list_mcp_servers(repo.manager())
                .await
                .map_err(Into::into),
        }
    }

    pub async fn list_mcp_servers_page(
        &self,
        user_id: Option<i64>,
        is_admin: bool,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<McpServer>)> {
        match self {
            Self::Postgres(repo) => {
                if is_admin {
                    crate::db::list_mcp_servers_page(repo.pool(), first, rows).await
                } else {
                    crate::db::list_user_mcp_servers_page(
                        repo.pool(),
                        user_id.expect("non-admin MCP requests require a user id"),
                        first,
                        rows,
                    )
                    .await
                }
            }
            Self::Sqlite(repo) => {
                if is_admin {
                    repo.store()
                        .list_mcp_servers_page(repo.manager(), first, rows)
                        .await
                        .map_err(Into::into)
                } else {
                    repo.store()
                        .list_user_mcp_servers_page(
                            repo.manager(),
                            user_id.expect("non-admin MCP requests require a user id"),
                            first,
                            rows,
                        )
                        .await
                        .map_err(Into::into)
                }
            }
        }
    }

    pub async fn list_visible_mcp_servers(&self, user_id: Option<i64>) -> Result<Vec<McpServer>> {
        match self {
            Self::Postgres(repo) => crate::db::list_visible_mcp_servers(repo.pool(), user_id).await,
            Self::Sqlite(repo) => repo
                .store()
                .list_visible_mcp_servers(repo.manager(), user_id)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn get_visible_mcp_server(
        &self,
        user_id: Option<i64>,
        name: &str,
    ) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::get_visible_mcp_server(repo.pool(), user_id, name).await
            }
            Self::Sqlite(repo) => {
                let servers = repo
                    .store()
                    .list_visible_mcp_servers(repo.manager(), user_id)
                    .await?;
                Ok(servers.into_iter().find(|server| server.name == name))
            }
        }
    }

    pub async fn get_mcp_server(&self, server_id: Uuid) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => crate::db::get_mcp_server(repo.pool(), server_id).await,
            Self::Sqlite(repo) => repo
                .store()
                .get_mcp_server(repo.manager(), server_id)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn get_user_mcp_server(
        &self,
        user_id: i64,
        server_id: Uuid,
    ) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::get_user_mcp_server(repo.pool(), user_id, server_id).await
            }
            Self::Sqlite(repo) => repo
                .store()
                .get_user_mcp_server(repo.manager(), user_id, server_id)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn get_mcp_server_by_name(&self, name: &str) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => crate::db::get_mcp_server_by_name(repo.pool(), name).await,
            Self::Sqlite(repo) => repo
                .store()
                .get_mcp_server_by_name(repo.manager(), name)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn get_mcp_server_by_source_endpoint(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::get_mcp_server_by_source_endpoint(repo.pool(), endpoint_id).await
            }
            Self::Sqlite(repo) => repo
                .store()
                .get_mcp_server_by_source_endpoint(repo.manager(), endpoint_id)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn create_mcp_server(
        &self,
        server_id: Uuid,
        input: McpServerInput,
    ) -> Result<McpServer> {
        match self {
            Self::Postgres(repo) => crate::db::create_mcp_server(repo.pool(), input).await,
            Self::Sqlite(repo) => repo
                .store()
                .save_mcp_server(repo.manager(), server_id, &input, None)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn update_mcp_server(
        &self,
        server_id: Uuid,
        input: McpServerInput,
    ) -> Result<Option<McpServer>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::update_mcp_server(repo.pool(), server_id, input).await
            }
            Self::Sqlite(repo) => {
                let existing = repo
                    .store()
                    .get_mcp_server(repo.manager(), server_id)
                    .await?;
                let Some(existing) = existing else {
                    return Ok(None);
                };
                repo.store()
                    .save_mcp_server(repo.manager(), server_id, &input, Some(&existing))
                    .await
                    .map(Some)
                    .map_err(Into::into)
            }
        }
    }

    pub async fn delete_mcp_server(&self, server_id: Uuid) -> Result<bool> {
        match self {
            Self::Postgres(repo) => crate::db::delete_mcp_server(repo.pool(), server_id).await,
            Self::Sqlite(repo) => repo
                .store()
                .delete_mcp_server(server_id)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn mark_mcp_lifecycle_learned(
        &self,
        server: &McpServer,
        mode: &str,
        protocol_version: &str,
    ) -> Result<bool> {
        match self {
            Self::Postgres(repo) => {
                crate::db::mark_mcp_lifecycle_learned(repo.pool(), server, mode, protocol_version)
                    .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .mark_mcp_lifecycle_learned(server, mode, protocol_version)
                .await
                .map_err(Into::into),
        }
    }

    pub async fn sync_minimax_mcp_server(
        &self,
        endpoint: &ProviderEndpoint,
        requested_enabled: bool,
    ) -> Result<()> {
        match self {
            Self::Postgres(repo) => {
                crate::db::sync_minimax_mcp_server(repo.pool(), endpoint, requested_enabled).await
            }
            Self::Sqlite(repo) => {
                let enabled = requested_enabled
                    && endpoint.enabled
                    && endpoint.provider == crate::db::EndpointProvider::Minimax;
                let existing = repo
                    .store()
                    .get_mcp_server_by_source_endpoint(repo.manager(), endpoint.endpoint_id)
                    .await?;
                if let Some(existing) = existing {
                    let mut input = input_from_endpoint(endpoint, existing.name.clone(), enabled);
                    input.source_endpoint_id = Some(endpoint.endpoint_id);
                    repo.store()
                        .save_mcp_server(
                            repo.manager(),
                            existing.server_id,
                            &input,
                            Some(&existing),
                        )
                        .await?;
                    return Ok(());
                }
                if endpoint.provider != crate::db::EndpointProvider::Minimax || !requested_enabled {
                    return Ok(());
                }
                let base_name = format!("{} MCP", endpoint.name.trim());
                let name = if repo
                    .store()
                    .get_mcp_server_by_name(repo.manager(), &base_name)
                    .await?
                    .is_none()
                {
                    base_name
                } else {
                    format!("{} MCP", endpoint.endpoint_id.simple())
                };
                let input = input_from_endpoint(endpoint, name, enabled);
                repo.store()
                    .save_mcp_server(repo.manager(), Uuid::new_v4(), &input, None)
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn list_mcp_credentials(
        &self,
        server_id: Uuid,
    ) -> Result<Vec<crate::db::McpCredential>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::list_credentials_by_server(repo.pool(), server_id).await
            }
            Self::Sqlite(repo) => {
                let Some(server) = repo
                    .store()
                    .get_mcp_server(repo.manager(), server_id)
                    .await?
                else {
                    return Ok(Vec::new());
                };
                let now = chrono::Utc::now();
                Ok(server
                    .bearer_tokens()
                    .into_iter()
                    .enumerate()
                    .map(|(index, token)| crate::db::McpCredential {
                        credential_id: Uuid::from_u128(
                            server.server_id.as_u128().wrapping_add(index as u128 + 1),
                        ),
                        server_id,
                        credential_label: format!("token-{}", index + 1),
                        secret: token.token,
                        position: index as i32,
                        enabled: token.enabled,
                        quota_group_id: None,
                        provider_kind: None,
                        daily_limit: None,
                        monthly_limit: None,
                        default_cost: 1.0,
                        strict_mode: false,
                        billing_period_start: None,
                        billing_period_end: None,
                        provider_remaining: None,
                        provider_synced_at: None,
                        provider_reset_at: None,
                        cooldown_until: None,
                        last_error: None,
                        last_error_at: None,
                        created_at: now,
                        updated_at: now,
                    })
                    .collect())
            }
        }
    }
}

fn input_from_endpoint(endpoint: &ProviderEndpoint, name: String, enabled: bool) -> McpServerInput {
    McpServerInput {
        scope: endpoint.scope.clone(),
        owner_user_id: endpoint.owner_user_id,
        source_endpoint_id: Some(endpoint.endpoint_id),
        name,
        aggregate_naming_mode: "passthrough_preferred".to_string(),
        transport: "builtin_minimax".to_string(),
        url: None,
        command: None,
        args: serde_json::json!([]),
        env_json: serde_json::json!({}),
        bearer_tokens_json: serde_json::json!([]),
        http_headers_json: serde_json::json!({}),
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        daily_max_requests: None,
        monthly_max_requests: None,
        enabled,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}
