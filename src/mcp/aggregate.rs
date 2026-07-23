use crate::db;
use serde_json::{Value, json};

use super::{
    McpCatalogCache,
    protocol::{aggregate_initialize_result, json_response},
    routing::{route_prefixed, route_resource},
    service,
};

pub(super) async fn aggregate(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    conversation_id: Option<&str>,
    request: Value,
) -> anyhow::Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "initialize" => Ok(json_response(id, aggregate_initialize_result()?)),
        "notifications/initialized" => Ok(Value::Null),
        "tools/list" => aggregate_tools(pool, cache, user_id, id).await,
        "resources/list" => aggregate_resources(pool, cache, user_id, id).await,
        "prompts/list" => aggregate_prompts(pool, cache, user_id, id).await,
        "tools/call" => {
            route_prefixed(
                pool,
                user_id,
                conversation_id,
                id,
                request,
                "name",
                "tools/call",
            )
            .await
        }
        "prompts/get" => {
            route_prefixed(
                pool,
                user_id,
                conversation_id,
                id,
                request,
                "name",
                "prompts/get",
            )
            .await
        }
        "resources/read" => route_resource(pool, user_id, conversation_id, id, request).await,
        _ => Ok(json_response(id, json!({}))),
    }
}

async fn aggregate_tools(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = db::list_visible_mcp_servers(pool, user_id).await?;
    let values = service::aggregate_tools(cache, &servers).await?;
    Ok(json_response(id, json!({ "tools": values })))
}

async fn aggregate_resources(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = db::list_visible_mcp_servers(pool, user_id).await?;
    let resources = service::aggregate_resources(cache, &servers).await?;
    Ok(json_response(id, json!({ "resources": resources })))
}

async fn aggregate_prompts(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = db::list_visible_mcp_servers(pool, user_id).await?;
    let prompts = service::aggregate_prompts(cache, &servers).await?;
    Ok(json_response(id, json!({ "prompts": prompts })))
}
