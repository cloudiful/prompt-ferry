use anyhow::Result;
use sea_orm::sea_query::{Expr, ExprTrait, NullOrdering, Order};
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use sqlx::PgPool;

use crate::db::entities::postgres::mcp_servers;
use crate::db::seaorm::{driver_error_details, native_error, postgres_connection};
use crate::db::types::{McpServer, McpServerInput};
use crate::db::{EndpointProvider, ProviderEndpoint};

use super::mcp_credentials::sync_credentials_from_tokens;

// Issue #375 Phase F: derive the saved-proxy indicator from the stored
// value; the secret itself stays `skip_serializing`. `has_proxy_url` is
// `#[sqlx(default)]` so legacy selects without the column still load.
fn with_proxy_indicator(mut server: McpServer) -> McpServer {
    server.has_proxy_url = server
        .proxy_url
        .as_deref()
        .is_some_and(|raw| !raw.trim().is_empty());
    server
}

fn server_from_model(model: mcp_servers::Model) -> McpServer {
    with_proxy_indicator(McpServer {
        server_id: model.server_id,
        source_endpoint_id: model.source_endpoint_id,
        scope: model.scope,
        owner_user_id: model.owner_user_id,
        name: model.name,
        aggregate_naming_mode: model.aggregate_naming_mode,
        transport: model.transport,
        provider_kind: model.provider_kind,
        url: model.url,
        command: model.command,
        args: model.args,
        env_json: model.env_json,
        bearer_tokens_json: model.bearer_tokens_json,
        http_headers_json: model.http_headers_json,
        auth_mode: model.auth_mode,
        basic_username: model.basic_username,
        basic_password: model.basic_password,
        proxy_url: model.proxy_url,
        has_proxy_url: false,
        tool_filter_mode: model.tool_filter_mode,
        allowed_tools: model.allowed_tools,
        disabled_tools: model.disabled_tools,
        disabled_resources: model.disabled_resources,
        enabled: model.enabled,
        timeout_ms: model.timeout_ms,
        lifecycle_policy: model.lifecycle_policy,
        lifecycle_manual_protocol_version: model.lifecycle_manual_protocol_version,
        lifecycle_learned_mode: model.lifecycle_learned_mode,
        lifecycle_learned_protocol_version: model.lifecycle_learned_protocol_version,
        lifecycle_learned_for_updated_at: model.lifecycle_learned_for_updated_at,
        lifecycle_learned_at: model.lifecycle_learned_at,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn active_model(input: McpServerInput) -> mcp_servers::ActiveModel {
    mcp_servers::ActiveModel {
        source_endpoint_id: Set(input.source_endpoint_id),
        scope: Set(input.scope),
        owner_user_id: Set(input.owner_user_id),
        name: Set(input.name),
        aggregate_naming_mode: Set(input.aggregate_naming_mode),
        transport: Set(input.transport),
        provider_kind: Set(input.provider_kind),
        url: Set(input.url),
        command: Set(input.command),
        args: Set(input.args),
        env_json: Set(input.env_json),
        bearer_tokens_json: Set(input.bearer_tokens_json),
        http_headers_json: Set(input.http_headers_json),
        auth_mode: Set(input.auth_mode),
        basic_username: Set(input.basic_username),
        basic_password: Set(input.basic_password),
        proxy_url: Set(input.proxy_url),
        tool_filter_mode: Set(input.tool_filter_mode),
        allowed_tools: Set(input.allowed_tools),
        disabled_tools: Set(input.disabled_tools),
        disabled_resources: Set(input.disabled_resources),
        enabled: Set(input.enabled),
        timeout_ms: Set(input.timeout_ms),
        lifecycle_policy: Set(input.lifecycle_policy),
        lifecycle_manual_protocol_version: Set(input.lifecycle_manual_protocol_version),
        ..Default::default()
    }
}

fn visible_to(user_id: Option<i64>) -> Condition {
    Condition::any()
        .add(
            Expr::col(mcp_servers::Column::Scope)
                .eq("admin")
                .and(Expr::col(mcp_servers::Column::OwnerUserId).is_null()),
        )
        .add(
            Expr::col(mcp_servers::Column::Scope)
                .eq("user")
                .and(Expr::col(mcp_servers::Column::OwnerUserId).eq(user_id)),
        )
}

pub async fn list_mcp_servers(pool: &PgPool) -> Result<Vec<McpServer>> {
    let db = postgres_connection(pool);
    let rows = mcp_servers::Entity::find()
        .order_by_desc(mcp_servers::Column::Scope)
        .order_by_with_nulls(
            mcp_servers::Column::OwnerUserId,
            Order::Asc,
            NullOrdering::First,
        )
        .order_by_asc(mcp_servers::Column::Name)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(server_from_model).collect())
}

pub async fn list_mcp_servers_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<McpServer>)> {
    let db = postgres_connection(pool);
    let total = mcp_servers::Entity::find()
        .count(&db)
        .await
        .map_err(native_error)? as i64;
    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let servers = mcp_servers::Entity::find()
        .order_by_desc(mcp_servers::Column::Scope)
        .order_by_with_nulls(
            mcp_servers::Column::OwnerUserId,
            Order::Asc,
            NullOrdering::First,
        )
        .order_by_asc(mcp_servers::Column::Name)
        .order_by_asc(mcp_servers::Column::ServerId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((total, servers.into_iter().map(server_from_model).collect()))
}

pub async fn list_visible_mcp_servers(
    pool: &PgPool,
    user_id: Option<i64>,
) -> Result<Vec<McpServer>> {
    let db = postgres_connection(pool);
    let rows = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Enabled.eq(true))
        .filter(visible_to(user_id))
        .order_by_asc(mcp_servers::Column::Name)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(server_from_model).collect())
}

pub async fn get_visible_mcp_server(
    pool: &PgPool,
    user_id: Option<i64>,
    name: &str,
) -> Result<Option<McpServer>> {
    let db = postgres_connection(pool);
    let row = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Name.eq(name))
        .filter(mcp_servers::Column::Enabled.eq(true))
        .filter(visible_to(user_id))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(server_from_model))
}

pub async fn get_mcp_server_by_name(pool: &PgPool, name: &str) -> Result<Option<McpServer>> {
    let db = postgres_connection(pool);
    let row = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Name.eq(name))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(server_from_model))
}

pub async fn get_mcp_server_by_source_endpoint(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
) -> Result<Option<McpServer>> {
    let db = postgres_connection(pool);
    let row = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::SourceEndpointId.eq(endpoint_id))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(server_from_model))
}

pub async fn list_user_mcp_servers(pool: &PgPool, user_id: i64) -> Result<Vec<McpServer>> {
    let db = postgres_connection(pool);
    let rows = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Scope.eq("user"))
        .filter(mcp_servers::Column::OwnerUserId.eq(user_id))
        .order_by_asc(mcp_servers::Column::Name)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(server_from_model).collect())
}

pub async fn list_user_mcp_servers_page(
    pool: &PgPool,
    user_id: i64,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<McpServer>)> {
    let db = postgres_connection(pool);
    let total = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Scope.eq("user"))
        .filter(mcp_servers::Column::OwnerUserId.eq(user_id))
        .count(&db)
        .await
        .map_err(native_error)? as i64;
    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let servers = mcp_servers::Entity::find()
        .filter(mcp_servers::Column::Scope.eq("user"))
        .filter(mcp_servers::Column::OwnerUserId.eq(user_id))
        .order_by_asc(mcp_servers::Column::Name)
        .order_by_asc(mcp_servers::Column::ServerId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((total, servers.into_iter().map(server_from_model).collect()))
}

pub async fn get_mcp_server(pool: &PgPool, server_id: uuid::Uuid) -> Result<Option<McpServer>> {
    let db = postgres_connection(pool);
    let row = mcp_servers::Entity::find_by_id(server_id)
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(server_from_model))
}

pub async fn get_user_mcp_server(
    pool: &PgPool,
    user_id: i64,
    server_id: uuid::Uuid,
) -> Result<Option<McpServer>> {
    let db = postgres_connection(pool);
    let row = mcp_servers::Entity::find_by_id(server_id)
        .filter(mcp_servers::Column::Scope.eq("user"))
        .filter(mcp_servers::Column::OwnerUserId.eq(user_id))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(server_from_model))
}

pub async fn create_mcp_server(pool: &PgPool, input: McpServerInput) -> Result<McpServer> {
    // Issue #375 Phase F: normalize empty proxy to NULL (inherit).
    let proxy_url = input
        .proxy_url
        .clone()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let db = postgres_connection(pool);
    let mut model = active_model(input);
    model.proxy_url = Set(proxy_url);
    let server = mcp_servers::Entity::insert(model)
        .exec_with_returning(&db)
        .await
        .map(server_from_model)
        .map_err(native_error)?;
    sync_credentials_from_tokens(pool, server.server_id, &server.bearer_tokens_json).await?;
    Ok(server)
}

pub async fn update_mcp_server(
    pool: &PgPool,
    server_id: uuid::Uuid,
    input: McpServerInput,
) -> Result<Option<McpServer>> {
    let proxy_url = input
        .proxy_url
        .clone()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let db = postgres_connection(pool);
    let mut model = active_model(input);
    model.proxy_url = Set(proxy_url);
    let server = mcp_servers::Entity::update_many()
        .set(model)
        .col_expr(mcp_servers::Column::UpdatedAt, Expr::current_timestamp())
        .filter(mcp_servers::Column::ServerId.eq(server_id))
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?
        .into_iter()
        .next()
        .map(server_from_model);
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
        let db = postgres_connection(pool);
        let updated = mcp_servers::Entity::update_many()
            .col_expr(
                mcp_servers::Column::Scope,
                Expr::value(endpoint.scope.clone()),
            )
            .col_expr(
                mcp_servers::Column::OwnerUserId,
                Expr::value(endpoint.owner_user_id),
            )
            .col_expr(mcp_servers::Column::Enabled, Expr::value(enabled))
            .col_expr(mcp_servers::Column::UpdatedAt, Expr::current_timestamp())
            .filter(mcp_servers::Column::ServerId.eq(server.server_id))
            .filter(mcp_servers::Column::SourceEndpointId.is_not_null())
            .exec_with_returning(&db)
            .await
            .map_err(native_error)?
            .into_iter()
            .next()
            .map(server_from_model);
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
                        let db = postgres_connection(pool);
                        let _ = mcp_servers::Entity::update_many()
                            .col_expr(
                                mcp_servers::Column::Scope,
                                Expr::value(endpoint.scope.clone()),
                            )
                            .col_expr(
                                mcp_servers::Column::OwnerUserId,
                                Expr::value(endpoint.owner_user_id),
                            )
                            .col_expr(mcp_servers::Column::Enabled, Expr::value(enabled))
                            .col_expr(mcp_servers::Column::UpdatedAt, Expr::current_timestamp())
                            .filter(mcp_servers::Column::ServerId.eq(server.server_id))
                            .filter(mcp_servers::Column::SourceEndpointId.is_not_null())
                            .exec_with_returning(&db)
                            .await
                            .map_err(native_error)?;
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
        provider_kind: Some(crate::db::MCP_PROVIDER_MINIMAX.to_string()),
        url: None,
        command: None,
        args: serde_json::json!([]),
        env_json: serde_json::json!({}),
        bearer_tokens_json: serde_json::json!([]),
        http_headers_json: serde_json::json!({}),
        auth_mode: crate::db::MCP_AUTH_MODE_NONE.to_string(),
        basic_username: None,
        basic_password: None,
        // Issue #375 Phase F: managed rows carry no row proxy; the builtin
        // transport inherits the source endpoint proxy at call time.
        proxy_url: None,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        enabled,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}

fn unique_violation_constraint(err: &anyhow::Error) -> Option<String> {
    let db_err = err.downcast_ref::<sea_orm::DbErr>()?;
    let details = driver_error_details(db_err)?;
    // PostgreSQL "unique_violation" is SQLSTATE 23505.
    if details.code.as_deref() != Some("23505") {
        return None;
    }
    details.constraint
}

pub async fn delete_mcp_server(pool: &PgPool, server_id: uuid::Uuid) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = mcp_servers::Entity::delete_many()
        .filter(mcp_servers::Column::ServerId.eq(server_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

pub async fn mark_mcp_lifecycle_learned(
    pool: &PgPool,
    server: &McpServer,
    mode: &str,
    protocol_version: &str,
) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = mcp_servers::Entity::update_many()
        .col_expr(mcp_servers::Column::LifecycleLearnedMode, Expr::value(mode))
        .col_expr(
            mcp_servers::Column::LifecycleLearnedProtocolVersion,
            Expr::value(protocol_version),
        )
        .col_expr(
            mcp_servers::Column::LifecycleLearnedForUpdatedAt,
            Expr::col(mcp_servers::Column::UpdatedAt),
        )
        .col_expr(
            mcp_servers::Column::LifecycleLearnedAt,
            Expr::current_timestamp(),
        )
        .filter(mcp_servers::Column::ServerId.eq(server.server_id))
        .filter(mcp_servers::Column::UpdatedAt.eq(server.updated_at))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}
