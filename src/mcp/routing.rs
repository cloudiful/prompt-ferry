use anyhow::anyhow;
use serde_json::Value;

use super::{
    filtering::{call_server, is_disabled_item},
    protocol::json_error_value,
    targeting::{
        load_visible_server, parse_prefixed_name, parse_resource_target,
        parse_resource_template_target,
    },
};

pub(super) async fn route_prefixed(
    pool: &sqlx::PgPool,
    user_id: Option<i64>,
    conversation_id: Option<&str>,
    id: Value,
    mut request: Value,
    field: &str,
    method: &str,
) -> anyhow::Result<Value> {
    let Some(name) = request
        .pointer(&format!("/params/{field}"))
        .and_then(Value::as_str)
    else {
        return Ok(json_error_value(id, -32602, "missing prefixed name"));
    };
    let Some(target) = parse_prefixed_name(name) else {
        return Ok(json_error_value(id, -32602, "name must be server__name"));
    };
    request["params"][field] = Value::String(target.upstream_name.clone());
    request["method"] = Value::String(method.to_string());
    let server = load_visible_server(pool, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    if method == "tools/call" && is_disabled_item(&server, "tools", &target.upstream_name) {
        return Ok(json_error_value(id, -32602, "tool is disabled"));
    }
    call_server(Some(pool), &server, request, conversation_id, None).await
}

pub(super) async fn route_resource(
    pool: &sqlx::PgPool,
    user_id: Option<i64>,
    conversation_id: Option<&str>,
    id: Value,
    mut request: Value,
) -> anyhow::Result<Value> {
    let Some(uri) = request.pointer("/params/uri").and_then(Value::as_str) else {
        return Ok(json_error_value(id, -32602, "missing resource uri"));
    };
    let Some(target) = parse_resource_target(uri)? else {
        return Ok(json_error_value(
            id,
            -32602,
            "uri must start with mcp://server/",
        ));
    };
    request["params"]["uri"] = Value::String(target.upstream_name.clone());
    let server = load_visible_server(pool, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    if is_disabled_item(&server, "resources", &target.upstream_name) {
        return Ok(json_error_value(id, -32602, "resource is disabled"));
    }
    call_server(Some(pool), &server, request, conversation_id, None).await
}

pub(super) async fn route_completion(
    pool: &sqlx::PgPool,
    user_id: Option<i64>,
    conversation_id: Option<&str>,
    id: Value,
    mut request: Value,
) -> anyhow::Result<Value> {
    let Some(reference_type) = request.pointer("/params/ref/type").and_then(Value::as_str) else {
        return Ok(json_error_value(id, -32602, "missing completion ref type"));
    };
    let target = match reference_type {
        "ref/prompt" => {
            let Some(name) = request.pointer("/params/ref/name").and_then(Value::as_str) else {
                return Ok(json_error_value(id, -32602, "missing completion ref name"));
            };
            let Some(target) = parse_prefixed_name(name) else {
                return Ok(json_error_value(
                    id,
                    -32602,
                    "ref/prompt name must be server__name",
                ));
            };
            request["params"]["ref"]["name"] = Value::String(target.upstream_name.clone());
            target
        }
        "ref/resource" => {
            let Some(uri) = request.pointer("/params/ref/uri").and_then(Value::as_str) else {
                return Ok(json_error_value(id, -32602, "missing completion ref uri"));
            };
            let Some(target) = parse_resource_template_target(uri)? else {
                return Ok(json_error_value(
                    id,
                    -32602,
                    "ref/resource uri must be a namespaced mcp:// template",
                ));
            };
            request["params"]["ref"]["uri"] = Value::String(target.upstream_name.clone());
            target
        }
        other => {
            return Ok(json_error_value(
                id,
                -32602,
                &format!("unsupported completion ref type: {other}"),
            ));
        }
    };
    request["method"] = Value::String("completion/complete".to_string());
    let server = load_visible_server(pool, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    call_server(Some(pool), &server, request, conversation_id, None).await
}
