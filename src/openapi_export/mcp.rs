use crate::{
    db,
    worker_admin_types::{
        CredentialPageResponse, CredentialQuotaBindingRequest, McpCatalogResponse,
        McpServerPageResponse, McpServerRequest, McpTestResponse, QuotaGroupRequest,
        QuotaGroupUsageResponse, TablePageQuery,
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-servers/{server_id}/credentials",
    params(("server_id" = uuid::Uuid, Path, description = "MCP server ID")),
    responses((status = 200, body = CredentialPageResponse, description = "Server credentials")),
    tag = "mcp"
)]
pub(super) fn list_server_credentials() {}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp-servers/{server_id}/credentials/{credential_id}/quota-group",
    params(
        ("server_id" = uuid::Uuid, Path, description = "MCP server ID"),
        ("credential_id" = uuid::Uuid, Path, description = "Credential ID"),
    ),
    request_body = CredentialQuotaBindingRequest,
    responses((status = 200, body = db::McpCredentialView, description = "Updated credential")),
    tag = "mcp"
)]
pub(super) fn bind_credential_group() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-quota-groups",
    responses((status = 200, body = Vec<db::McpQuotaGroup>, description = "Quota groups")),
    tag = "mcp"
)]
pub(super) fn list_quota_groups() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp-quota-groups",
    request_body = QuotaGroupRequest,
    responses((status = 200, body = db::McpQuotaGroup, description = "Created quota group")),
    tag = "mcp"
)]
pub(super) fn create_quota_group() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/mcp-quota-groups/{group_id}",
    params(("group_id" = uuid::Uuid, Path, description = "Quota group ID")),
    request_body = QuotaGroupRequest,
    responses((status = 200, body = db::McpQuotaGroup, description = "Updated quota group")),
    tag = "mcp"
)]
pub(super) fn update_quota_group() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp-quota-groups/{group_id}",
    params(("group_id" = uuid::Uuid, Path, description = "Quota group ID")),
    responses((status = 204, description = "Deleted quota group")),
    tag = "mcp"
)]
pub(super) fn delete_quota_group() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp-quota-groups/{group_id}/usage",
    params(("group_id" = uuid::Uuid, Path, description = "Quota group ID")),
    responses((status = 200, body = QuotaGroupUsageResponse, description = "Quota group usage")),
    tag = "mcp"
)]
pub(super) fn quota_group_usage() {}
