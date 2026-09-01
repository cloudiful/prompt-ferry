use anyhow::Result;
use sqlx::PgPool;

use crate::db::types::{McpServer, McpServerInput};
use crate::db::{EndpointProvider, ProviderEndpoint};

use super::mcp_credentials::sync_credentials_from_tokens;

pub async fn list_mcp_servers(pool: &PgPool) -> Result<Vec<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/list_mcp_servers.sql",)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn list_mcp_servers_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<McpServer>)> {
    let total = sqlx::query_file!("src/sql/mcp/count_mcp_servers.sql")
        .fetch_one(pool)
        .await?
        .total;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let servers = sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/list_mcp_servers_page.sql",
        first,
        rows,
    )
    .fetch_all(pool)
    .await?;
    Ok((total, servers))
}

pub async fn list_visible_mcp_servers(
    pool: &PgPool,
    user_id: Option<i64>,
) -> Result<Vec<McpServer>> {
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/list_visible_mcp_servers.sql",
        user_id,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_visible_mcp_server(
    pool: &PgPool,
    user_id: Option<i64>,
    name: &str,
) -> Result<Option<McpServer>> {
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/get_visible_mcp_server.sql",
        user_id,
        name,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_mcp_server_by_name(pool: &PgPool, name: &str) -> Result<Option<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/get_mcp_server_by_name.sql", name,)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn get_mcp_server_by_source_endpoint(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
) -> Result<Option<McpServer>> {
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/get_mcp_server_by_source_endpoint.sql",
        endpoint_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn list_user_mcp_servers(pool: &PgPool, user_id: i64) -> Result<Vec<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/list_user_mcp_servers.sql", user_id,)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn list_user_mcp_servers_page(
    pool: &PgPool,
    user_id: i64,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<McpServer>)> {
    let total = sqlx::query_file!("src/sql/mcp/count_user_mcp_servers.sql", user_id)
        .fetch_one(pool)
        .await?
        .total;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let servers = sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/list_user_mcp_servers_page.sql",
        user_id,
        first,
        rows,
    )
    .fetch_all(pool)
    .await?;
    Ok((total, servers))
}

pub async fn get_mcp_server(pool: &PgPool, server_id: uuid::Uuid) -> Result<Option<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/get_mcp_server.sql", server_id,)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn get_user_mcp_server(
    pool: &PgPool,
    user_id: i64,
    server_id: uuid::Uuid,
) -> Result<Option<McpServer>> {
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/get_user_mcp_server.sql",
        server_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn create_mcp_server(pool: &PgPool, input: McpServerInput) -> Result<McpServer> {
    let server = sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/create_mcp_server.sql",
        input.scope,
        input.owner_user_id,
        input.source_endpoint_id,
        input.name,
        input.aggregate_naming_mode,
        input.transport,
        input.url,
        input.command,
        input.args,
        input.env_json,
        input.bearer_tokens_json,
        input.http_headers_json,
        input.auth_mode,
        input.basic_username,
        input.basic_password,
        input.tool_filter_mode,
        input.allowed_tools,
        input.disabled_tools,
        input.disabled_resources,
        input.daily_max_requests,
        input.monthly_max_requests,
        input.enabled,
        input.timeout_ms,
        input.lifecycle_policy,
        input.lifecycle_manual_protocol_version,
    )
    .fetch_one(pool)
    .await?;
    sync_credentials_from_tokens(pool, server.server_id, &server.bearer_tokens_json).await?;
    Ok(server)
}

pub async fn update_mcp_server(
    pool: &PgPool,
    server_id: uuid::Uuid,
    input: McpServerInput,
) -> Result<Option<McpServer>> {
    let server = sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/update_mcp_server.sql",
        server_id,
        input.scope,
        input.owner_user_id,
        input.source_endpoint_id,
        input.name,
        input.aggregate_naming_mode,
        input.transport,
        input.url,
        input.command,
        input.args,
        input.env_json,
        input.bearer_tokens_json,
        input.http_headers_json,
        input.auth_mode,
        input.basic_username,
        input.basic_password,
        input.tool_filter_mode,
        input.allowed_tools,
        input.disabled_tools,
        input.disabled_resources,
        input.daily_max_requests,
        input.monthly_max_requests,
        input.enabled,
        input.timeout_ms,
        input.lifecycle_policy,
        input.lifecycle_manual_protocol_version,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(server) = server.as_ref() {
        sync_credentials_from_tokens(pool, server.server_id, &server.bearer_tokens_json).await?;
    }
    Ok(server)
}

pub async fn sync_minimax_mcp_server(
    pool: &PgPool,
    endpoint: &ProviderEndpoint,
    requested_enabled: bool,
) -> Result<()> {
    let existing = get_mcp_server_by_source_endpoint(pool, endpoint.endpoint_id).await?;
    let enabled =
        requested_enabled && endpoint.enabled && endpoint.provider == EndpointProvider::Minimax;
    if let Some(server) = existing {
        let updated = sqlx::query_file_as!(
            McpServer,
            "src/sql/mcp/update_managed_mcp_server.sql",
            server.server_id,
            endpoint.scope,
            endpoint.owner_user_id,
            enabled,
        )
        .fetch_optional(pool)
        .await?;
        if updated.is_some() {
            return Ok(());
        }
        // The managed row is gone (e.g. concurrent delete) - fall through to
        // recreate it below instead of silently leaving the endpoint without
        // its managed MCP server.
    }
    if endpoint.provider != EndpointProvider::Minimax || !requested_enabled {
        return Ok(());
    }
    create_managed_mcp_server(pool, endpoint, enabled).await
}

/// Insert the managed MCP row for a MiniMax endpoint, recovering from the
/// two unique-violation races that can occur when two endpoints share an
/// owner-visible name: another worker may have inserted the same source
/// endpoint or the same name first. Any other database error is propagated
/// unchanged so the caller can surface it as an internal failure.
async fn create_managed_mcp_server(
    pool: &PgPool,
    endpoint: &ProviderEndpoint,
    enabled: bool,
) -> Result<()> {
    let base_name = format!("{} MCP", endpoint.name.trim());
    let fallback_name = format!("{} MCP", endpoint.endpoint_id.simple());
    let initial_name = if get_mcp_server_by_name(pool, &base_name).await?.is_none() {
        base_name
    } else {
        fallback_name.clone()
    };

    let mut name = initial_name;
    let mut used_fallback = name == fallback_name;
    loop {
        match create_mcp_server(
            pool,
            managed_mcp_server_input(endpoint, name.clone(), enabled),
        )
        .await
        {
            Ok(_) => return Ok(()),
            Err(err) => {
                let Some(constraint) = unique_violation_constraint(&err) else {
                    return Err(err);
                };
                if constraint == "idx_mcp_servers_source_endpoint" {
                    // A concurrent insert won the race for this source
                    // endpoint. Re-read the existing row and apply the
                    // requested enabled/scope via the managed-updater.
                    if let Some(server) =
                        get_mcp_server_by_source_endpoint(pool, endpoint.endpoint_id).await?
                    {
                        let _ = sqlx::query_file_as!(
                            McpServer,
                            "src/sql/mcp/update_managed_mcp_server.sql",
                            server.server_id,
                            endpoint.scope,
                            endpoint.owner_user_id,
                            enabled,
                        )
                        .fetch_optional(pool)
                        .await?;
                        return Ok(());
                    }
                    return Err(err);
                }
                if constraint == "mcp_servers_name_key" {
                    if used_fallback {
                        // The deterministic UUID-based name is unique per
                        // endpoint, so a collision here means an unrelated
                        // row grabbed it. Surface the original error rather
                        // than loop forever.
                        return Err(err);
                    }
                    name = fallback_name.clone();
                    used_fallback = true;
                    continue;
                }
                return Err(err);
            }
        }
    }
}

fn managed_mcp_server_input(
    endpoint: &ProviderEndpoint,
    name: String,
    enabled: bool,
) -> McpServerInput {
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
        auth_mode: crate::db::MCP_AUTH_MODE_NONE.to_string(),
        basic_username: None,
        basic_password: None,
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

fn unique_violation_constraint(err: &anyhow::Error) -> Option<&str> {
    let db_err = err.downcast_ref::<sqlx::Error>()?;
    let sqlx::Error::Database(db_err) = db_err else {
        return None;
    };
    // PostgreSQL "unique_violation" is SQLSTATE 23505.
    if db_err.code().as_deref() != Some("23505") {
        return None;
    }
    db_err.constraint()
}

pub async fn delete_mcp_server(pool: &PgPool, server_id: uuid::Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/mcp/delete_mcp_server.sql", server_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn mark_mcp_lifecycle_learned(
    pool: &PgPool,
    server: &McpServer,
    mode: &str,
    protocol_version: &str,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/mcp/mark_lifecycle_learned.sql",
        server.server_id,
        mode,
        protocol_version,
        server.updated_at,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
