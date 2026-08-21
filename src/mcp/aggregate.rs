use serde_json::{Value, json};

use super::{
    McpCatalogCache, McpRuntimeStorage,
    protocol::{aggregate_initialize_result, json_response},
    routing::{route_completion, route_prefixed, route_resource},
    service,
};

pub(super) async fn aggregate(
    storage: &McpRuntimeStorage,
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
        "tools/list" => aggregate_tools(storage, cache, user_id, id).await,
        "resources/list" => aggregate_resources(storage, cache, user_id, id).await,
        "resources/templates/list" => aggregate_templates(storage, cache, user_id, id).await,
        "prompts/list" => aggregate_prompts(storage, cache, user_id, id).await,
        "tools/call" => {
            route_prefixed(
                storage,
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
                storage,
                user_id,
                conversation_id,
                id,
                request,
                "name",
                "prompts/get",
            )
            .await
        }
        "resources/read" => route_resource(storage, user_id, conversation_id, id, request).await,
        "completion/complete" => {
            route_completion(storage, user_id, conversation_id, id, request).await
        }
        _ => Ok(json_response(id, json!({}))),
    }
}

async fn aggregate_tools(
    storage: &McpRuntimeStorage,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = storage
        .repository()
        .list_visible_mcp_servers(user_id)
        .await?;
    let values = service::aggregate_tools(cache, &servers).await?;
    Ok(json_response(id, json!({ "tools": values })))
}

async fn aggregate_resources(
    storage: &McpRuntimeStorage,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = storage
        .repository()
        .list_visible_mcp_servers(user_id)
        .await?;
    let resources = service::aggregate_resources(cache, &servers).await?;
    Ok(json_response(id, json!({ "resources": resources })))
}

async fn aggregate_templates(
    storage: &McpRuntimeStorage,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = storage
        .repository()
        .list_visible_mcp_servers(user_id)
        .await?;
    let templates = service::aggregate_resource_templates(cache, &servers).await?;
    Ok(json_response(id, json!({ "resourceTemplates": templates })))
}

async fn aggregate_prompts(
    storage: &McpRuntimeStorage,
    cache: &McpCatalogCache,
    user_id: Option<i64>,
    id: Value,
) -> anyhow::Result<Value> {
    let servers = storage
        .repository()
        .list_visible_mcp_servers(user_id)
        .await?;
    let prompts = service::aggregate_prompts(cache, &servers).await?;
    Ok(json_response(id, json!({ "prompts": prompts })))
}
