use crate::db::{self, McpServer};
use serde_json::Value;

use super::protocol::decode_resource_uri;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpRequestMetadata {
    pub server_name: Option<String>,
    pub protocol_method: Option<String>,
    pub operation_name: Option<String>,
    pub selected_token_slot: Option<i16>,
    pub request_raw_json: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixedTarget {
    pub server_name: String,
    pub upstream_name: String,
}

pub fn parse_prefixed_name(name: &str) -> Option<PrefixedTarget> {
    name.split_once("__")
        .map(|(server_name, upstream_name)| PrefixedTarget {
            server_name: server_name.to_string(),
            upstream_name: upstream_name.to_string(),
        })
}

pub fn parse_resource_target(uri: &str) -> anyhow::Result<Option<PrefixedTarget>> {
    decode_resource_uri(uri).map(|value| {
        value.map(|(server_name, upstream_name)| PrefixedTarget {
            server_name,
            upstream_name,
        })
    })
}

pub fn extract_mcp_request_metadata(
    explicit_server_name: Option<&str>,
    headers: &[(String, String)],
    body: &[u8],
) -> McpRequestMetadata {
    let body_value = serde_json::from_slice(body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).to_string()));
    let protocol_method = body_value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let params = body_value.get("params");
    let mut server_name = explicit_server_name.map(str::to_string);
    let operation_name = match protocol_method.as_deref() {
        Some("tools/call") | Some("prompts/get") => params
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .map(|name| {
                if server_name.is_none()
                    && let Some(target) = parse_prefixed_name(name)
                {
                    server_name = Some(target.server_name);
                    return target.upstream_name;
                }
                name.to_string()
            }),
        Some("resources/read") => params
            .and_then(|value| value.get("uri"))
            .and_then(Value::as_str)
            .map(|uri| {
                if server_name.is_none()
                    && let Ok(Some(target)) = parse_resource_target(uri)
                {
                    server_name = Some(target.server_name);
                    return target.upstream_name;
                }
                uri.to_string()
            }),
        _ => None,
    };

    let request_raw_json = serde_json::json!({
        "headers": headers
            .iter()
            .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
            .collect::<Vec<_>>(),
        "body": body_value,
    });

    McpRequestMetadata {
        server_name,
        protocol_method,
        operation_name,
        selected_token_slot: None,
        request_raw_json: Some(request_raw_json),
    }
}

pub async fn load_visible_server(
    pool: &sqlx::PgPool,
    user_id: Option<i64>,
    server_name: &str,
) -> anyhow::Result<Option<McpServer>> {
    db::get_visible_mcp_server(pool, user_id, server_name).await
}

#[cfg(test)]
mod tests {
    use super::{extract_mcp_request_metadata, parse_resource_target};

    #[test]
    fn extracts_aggregate_tool_target() {
        let metadata = extract_mcp_request_metadata(
            None,
            &[],
            br#"{"method":"tools/call","params":{"name":"ctx__search"}}"#,
        );
        assert_eq!(metadata.server_name.as_deref(), Some("ctx"));
        assert_eq!(metadata.operation_name.as_deref(), Some("search"));
    }

    #[test]
    fn parses_resource_target_round_trip() {
        let target = parse_resource_target("mcp://ctx/file%3A%2F%2F%2Ftmp%2Fa").unwrap();
        assert_eq!(target.expect("target").server_name, "ctx");
    }
}
