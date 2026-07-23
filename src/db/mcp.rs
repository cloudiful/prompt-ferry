use anyhow::Result;
use sqlx::PgPool;

use crate::db::types::{McpServer, McpServerInput};

pub async fn list_mcp_servers(pool: &PgPool) -> Result<Vec<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/list_mcp_servers.sql",)
            .fetch_all(pool)
            .await?,
    )
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

pub async fn list_user_mcp_servers(pool: &PgPool, user_id: i64) -> Result<Vec<McpServer>> {
    Ok(
        sqlx::query_file_as!(McpServer, "src/sql/mcp/list_user_mcp_servers.sql", user_id,)
            .fetch_all(pool)
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
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/create_mcp_server.sql",
        input.scope,
        input.owner_user_id,
        input.name,
        input.aggregate_naming_mode,
        input.transport,
        input.url,
        input.command,
        input.args,
        input.env_json,
        input.bearer_tokens_json,
        input.http_headers_json,
        input.tool_filter_mode,
        input.allowed_tools,
        input.disabled_tools,
        input.disabled_resources,
        input.daily_max_requests,
        input.monthly_max_requests,
        input.enabled,
        input.timeout_ms,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_mcp_server(
    pool: &PgPool,
    server_id: uuid::Uuid,
    input: McpServerInput,
) -> Result<Option<McpServer>> {
    Ok(sqlx::query_file_as!(
        McpServer,
        "src/sql/mcp/update_mcp_server.sql",
        server_id,
        input.scope,
        input.owner_user_id,
        input.name,
        input.aggregate_naming_mode,
        input.transport,
        input.url,
        input.command,
        input.args,
        input.env_json,
        input.bearer_tokens_json,
        input.http_headers_json,
        input.tool_filter_mode,
        input.allowed_tools,
        input.disabled_tools,
        input.disabled_resources,
        input.daily_max_requests,
        input.monthly_max_requests,
        input.enabled,
        input.timeout_ms,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_mcp_server(pool: &PgPool, server_id: uuid::Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/mcp/delete_mcp_server.sql", server_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
