use anyhow::anyhow;
use serde_json::Value;

use super::{
    McpRuntimeStorage,
    filtering::{call_server, is_disabled_item, is_tool_allowed},
    protocol::json_error_value,
    targeting::{
        load_visible_server, parse_prefixed_name, parse_resource_target,
        parse_resource_template_target,
    },
};

pub(super) async fn route_prefixed(
    storage: &McpRuntimeStorage,
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
    let server = load_visible_server(storage, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    if method == "tools/call" && !is_tool_allowed(&server, &target.upstream_name) {
        return Ok(json_error_value(id, -32602, "tool is not allowed"));
    }
    call_server(storage, &server, request, conversation_id, None).await
}

pub(super) async fn route_resource(
    storage: &McpRuntimeStorage,
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
    let server = load_visible_server(storage, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    if is_disabled_item(&server, "resources", &target.upstream_name) {
        return Ok(json_error_value(id, -32602, "resource is disabled"));
    }
    call_server(storage, &server, request, conversation_id, None).await
}

pub(super) async fn route_completion(
    storage: &McpRuntimeStorage,
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
    let server = load_visible_server(storage, user_id, &target.server_name)
        .await?
        .ok_or_else(|| anyhow!("mcp server not found or disabled"))?;
    call_server(storage, &server, request, conversation_id, None).await
}

#[cfg(test)]
mod tests {
    use super::is_tool_allowed;
    use crate::db::McpServer;
    use serde_json::{Value, json};

    fn server(tool_filter_mode: &str, allowed_tools: Value, disabled_tools: Value) -> McpServer {
        McpServer {
            server_id: uuid::Uuid::nil(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "test".to_string(),
            aggregate_naming_mode: "qualified_only".to_string(),
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: json!([]),
            env_json: json!({}),
            bearer_tokens_json: json!([]),
            http_headers_json: json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            tool_filter_mode: tool_filter_mode.to_string(),
            allowed_tools,
            disabled_tools,
            disabled_resources: json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
            lifecycle_learned_mode: None,
            lifecycle_learned_protocol_version: None,
            lifecycle_learned_for_updated_at: None,
            lifecycle_learned_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // Guards `route_prefixed` tools/call gate: whitelist must only pass allowed tools.
    #[test]
    fn prefixed_tools_call_whitelist_gate() {
        let server = server("whitelist", json!(["allowed_tool"]), json!([]));
        assert!(is_tool_allowed(&server, "allowed_tool"));
        assert!(!is_tool_allowed(&server, "other_tool"));
    }

    // Guards `route_prefixed` tools/call gate: blacklist keeps disabled negation.
    #[test]
    fn prefixed_tools_call_blacklist_gate() {
        let server = server("blacklist", json!([]), json!(["disabled_tool"]));
        assert!(is_tool_allowed(&server, "public_tool"));
        assert!(!is_tool_allowed(&server, "disabled_tool"));
    }
}
