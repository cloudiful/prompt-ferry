use anyhow::anyhow;
use serde_json::Value;

use crate::db::McpServer;

use super::{
    McpRuntimeStorage,
    filtering::{call_server, is_disabled_item, is_tool_allowed},
    protocol::json_error_value,
    targeting::{
        load_visible_server, parse_prefixed_name, parse_resource_target,
        parse_resource_template_target,
    },
};

/// Shared `tools/call` allowlist gate used by [`route_prefixed`].
///
/// Returns the `-32602 "tool is not allowed"` envelope when `upstream_name`
/// is filtered out, otherwise `None` meaning the caller may proceed to
/// `call_server`. Keeping the predicate and the envelope in one helper lets
/// unit tests pin the exact rejection shape on the same code path production
/// executes.
fn tools_call_gate_error(server: &McpServer, upstream_name: &str, id: Value) -> Option<Value> {
    if is_tool_allowed(server, upstream_name) {
        None
    } else {
        Some(json_error_value(id, -32602, "tool is not allowed"))
    }
}

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
    if method == "tools/call"
        && let Some(denied) = tools_call_gate_error(&server, &target.upstream_name, id)
    {
        return Ok(denied);
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
    use super::{json_error_value, parse_prefixed_name, tools_call_gate_error};
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

    fn tools_call_request(prefixed_name: &str, id: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": prefixed_name, "arguments": {} }
        })
    }

    /// Replays the exact `route_prefixed` tools/call prefix steps verbatim:
    /// `pointer("/params/name")` -> `parse_prefixed_name` ->
    /// `tools_call_gate_error` (the same helper production calls).
    fn gate_for_prefixed_request(
        server: &McpServer,
        request: &Value,
        id: Value,
    ) -> Option<Value> {
        let name = request
            .pointer("/params/name")
            .and_then(Value::as_str)
            .expect("synthesized tools/call request must carry params.name");
        let target =
            parse_prefixed_name(name).expect("prefixed tool name must be server__tool");
        tools_call_gate_error(server, &target.upstream_name, id)
    }

    fn assert_not_allowed_envelope(denied: &Value, id: &Value) {
        assert_eq!(denied["jsonrpc"], json!("2.0"));
        assert_eq!(denied["id"], *id);
        assert_eq!(denied["error"]["code"], json!(-32602));
        assert_eq!(denied["error"]["message"], json!("tool is not allowed"));
        assert_eq!(
            *denied,
            json_error_value(id.clone(), -32602, "tool is not allowed")
        );
    }

    // Inference chain (why this proves "never reaches upstream" without a live
    // `McpRuntimeStorage`/transport mock, and why it is stronger than asserting
    // `is_tool_allowed` alone):
    // `route_prefixed` runs `pointer("/params/name")` -> `parse_prefixed_name`
    // -> `tools_call_gate_error` and early-`return Ok(denied)` BEFORE the
    // trailing `call_server(storage, ...).await` line. The tests below drive
    // those same three steps on a synthesized JSON-RPC `tools/call`
    // `server__tool` request through the SAME `tools_call_gate_error`
    // production uses, and pin the full `-32602` envelope (code + message + id
    // echo). A `Some(envelope)` therefore locks the early-return branch that
    // cannot fall through to `call_server`/upstream; `None` locks the allow
    // branch that does fall through. Changing either the predicate or the
    // envelope text/code in production fails here.
    #[test]
    fn prefixed_tools_call_whitelist_gate() {
        let srv = server("whitelist", json!(["allowed_tool"]), json!([]));

        // Whitelist outside: must reject with the -32602 envelope.
        let id = json!(7);
        let request = tools_call_request("test__other_tool", id.clone());
        let denied = gate_for_prefixed_request(&srv, &request, id.clone())
            .expect("whitelist must reject a non-allowlisted tool");
        assert_not_allowed_envelope(&denied, &id);

        // Whitelist inside: must allow (fall through to `call_server`).
        let allow_id = json!(8);
        let allow_request = tools_call_request("test__allowed_tool", allow_id.clone());
        assert!(
            gate_for_prefixed_request(&srv, &allow_request, allow_id).is_none(),
            "whitelist must allow an allowlisted tool"
        );
    }

    // See the inference-chain comment above: `Some(-32602 envelope)` pins the
    // early return before `call_server`; `None` pins fall-through.
    #[test]
    fn prefixed_tools_call_blacklist_gate() {
        let srv = server("blacklist", json!([]), json!(["disabled_tool"]));

        // Blacklist disabled: must reject with the -32602 envelope.
        let id = json!(9);
        let request = tools_call_request("test__disabled_tool", id.clone());
        let denied = gate_for_prefixed_request(&srv, &request, id.clone())
            .expect("blacklist must reject a disabled tool");
        assert_not_allowed_envelope(&denied, &id);

        // Blacklist non-disabled: must allow (fall through to `call_server`).
        let allow_id = json!(10);
        let allow_request = tools_call_request("test__public_tool", allow_id.clone());
        assert!(
            gate_for_prefixed_request(&srv, &allow_request, allow_id).is_none(),
            "blacklist must allow a non-disabled tool"
        );
    }
}
