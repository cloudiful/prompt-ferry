use crate::{
    db,
    worker_admin_types::{
        McpCatalogResponse, McpServerPageResponse, McpServerRequest, McpTestResponse,
        TablePageQuery,
    },
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-servers",
    params(TablePageQuery),
    responses((status = 200, body = McpServerPageResponse, description = "MCP servers")),
    tag = "mcp"
)]
pub(super) fn list_mcp_servers() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp-servers",
    request_body = McpServerRequest,
    responses((status = 200, body = db::McpServer, description = "Created MCP server")),
    tag = "mcp"
)]
pub(super) fn create_mcp_server() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/mcp-servers/{server_id}",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    request_body = McpServerRequest,
    responses((status = 200, body = db::McpServer, description = "Updated MCP server")),
    tag = "mcp"
)]
pub(super) fn update_mcp_server() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp-servers/{server_id}",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 204, description = "Deleted MCP server")),
    tag = "mcp"
)]
pub(super) fn delete_mcp_server() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-servers/{server_id}/catalog",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = McpCatalogResponse, description = "Server catalog")),
    tag = "mcp"
)]
pub(super) fn get_mcp_catalog() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp-servers/{server_id}/test",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = McpTestResponse, description = "MCP server test result")),
    tag = "mcp"
)]
pub(super) fn test_mcp_server() {}
