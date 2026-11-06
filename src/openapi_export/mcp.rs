use crate::worker_admin_types::{
    McpCatalogResponse, McpOAuthCallbackQuery, McpOAuthRefreshResponse, McpOAuthStartResponse,
    McpOAuthStatusResponse, McpProviderDescriptor, McpServer, McpServerPageResponse,
    McpServerRequest, McpTestResponse, TablePageQuery,
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-providers",
    responses((status = 200, body = Vec<McpProviderDescriptor>, description = "MCP provider presets")),
    tag = "mcp"
)]
pub(super) fn list_mcp_providers() {}

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
    responses((status = 200, body = McpServer, description = "Created MCP server")),
    tag = "mcp"
)]
pub(super) fn create_mcp_server() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/mcp-servers/{server_id}",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    request_body = McpServerRequest,
    responses((status = 200, body = McpServer, description = "Updated MCP server")),
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-servers/{server_id}/oauth",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = McpOAuthStatusResponse, description = "Personal MCP OAuth status")),
    tag = "mcp"
)]
pub(super) fn get_mcp_oauth_status() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp-servers/{server_id}/oauth",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = McpOAuthStartResponse, description = "Authorization URL to visit")),
    tag = "mcp"
)]
pub(super) fn start_mcp_oauth() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-servers/{server_id}/oauth/callback",
    params(
        ("server_id" = uuid::Uuid, Path, description = "MCP server ID"),
        McpOAuthCallbackQuery
    ),
    responses((status = 200, description = "Fixed authorization result page")),
    tag = "mcp"
)]
pub(super) fn mcp_oauth_callback() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp-servers/{server_id}/oauth/refresh",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = McpOAuthRefreshResponse, description = "Refreshed grant status")),
    tag = "mcp"
)]
pub(super) fn refresh_mcp_oauth() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp-servers/{server_id}/oauth",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 204, description = "Grant, pending states and leases cleared")),
    tag = "mcp"
)]
pub(super) fn disconnect_mcp_oauth() {}
