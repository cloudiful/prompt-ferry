use serde_json::Value;
use std::collections::HashSet;

use crate::db::McpServer;

use super::{protocol::json_error_value, transport};

pub(super) async fn call_server(
    pool: Option<&sqlx::PgPool>,
    server: &McpServer,
    request: Value,
    conversation_id: Option<&str>,
    forced: Option<&crate::db::McpCredential>,
) -> anyhow::Result<Value> {
    transport::call_with_pool(pool, server, request, conversation_id, forced).await
}

pub(super) async fn call_server_filtered(
    pool: Option<&sqlx::PgPool>,
    server: &McpServer,
    request: Value,
    conversation_id: Option<&str>,
    forced: Option<&crate::db::McpCredential>,
) -> anyhow::Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if method == "tools/call"
        && let Some(name) = request.pointer("/params/name").and_then(Value::as_str)
        && !is_tool_allowed(server, name)
    {
        return Ok(json_error_value(id, -32602, "tool is not allowed"));
    }
    if method == "resources/read"
        && let Some(uri) = request.pointer("/params/uri").and_then(Value::as_str)
        && is_disabled_item(server, "resources", uri)
    {
        return Ok(json_error_value(id, -32602, "resource is disabled"));
    }
    let mut response = call_server(pool, server, request, conversation_id, forced).await?;
    if method == "tools/list" {
        filter_tool_items(&mut response, server, "name");
    } else if method == "resources/list" {
        filter_result_items(&mut response, server, "resources", "uri");
    }
    Ok(response)
}

pub(super) fn is_tool_allowed(server: &McpServer, name: &str) -> bool {
    match server.tool_filter_mode.as_str() {
        "whitelist" => string_set(&server.allowed_tools).contains(name),
        _ => !string_set(&server.disabled_tools).contains(name),
    }
}

pub(super) fn is_disabled_item(server: &McpServer, kind: &str, name: &str) -> bool {
    string_set(match kind {
        "tools" => &server.disabled_tools,
        "resources" => &server.disabled_resources,
        _ => return false,
    })
    .contains(name)
}

fn filter_tool_items(response: &mut Value, server: &McpServer, name_key: &str) {
    let Some(items) = response
        .pointer_mut("/result/tools")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    items.retain(|item| {
        item.get(name_key)
            .and_then(Value::as_str)
            .is_none_or(|name| is_tool_allowed(server, name))
    });
}

fn filter_result_items(response: &mut Value, server: &McpServer, kind: &str, name_key: &str) {
    let Some(items) = response
        .pointer_mut(&format!("/result/{kind}"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    items.retain(|item| {
        item.get(name_key)
            .and_then(Value::as_str)
            .is_none_or(|name| !is_disabled_item(server, kind, name))
    });
}

fn string_set(value: &Value) -> HashSet<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(tool_filter_mode: &str, allowed_tools: Value, disabled_tools: Value) -> McpServer {
        McpServer {
            server_id: uuid::Uuid::nil(),
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

    #[test]
    fn blacklist_mode_blocks_selected_tools() {
        let server = server("blacklist", json!([]), json!(["secret_tool"]));
        assert!(is_tool_allowed(&server, "public_tool"));
        assert!(!is_tool_allowed(&server, "secret_tool"));
    }

    #[test]
    fn whitelist_mode_only_allows_selected_tools() {
        let server = server("whitelist", json!(["public_tool"]), json!(["ignored"]));
        assert!(is_tool_allowed(&server, "public_tool"));
        assert!(!is_tool_allowed(&server, "secret_tool"));
    }
}
