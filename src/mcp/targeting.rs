use crate::db::{self, McpServer};
use serde_json::Value;

use super::protocol::{decode_resource_template_uri, decode_resource_uri};

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

/// Resolves a namespaced resource *template* URI (`mcp://server/{...}`) back
/// to its server and upstream template, preserving RFC 6570 expressions.
pub fn parse_resource_template_target(uri: &str) -> anyhow::Result<Option<PrefixedTarget>> {
    decode_resource_template_uri(uri).map(|value| {
        value.map(|(server_name, upstream_name)| PrefixedTarget {
            server_name,
            upstream_name,
        })
    })
}

/// Header names that must never reach the MCP worker or raw usage logs:
/// credentials, cookies, and hop-by-hop headers.
///
/// This list is intentionally narrower than [`RESERVED_MCP_HTTP_HEADERS`]
/// (which governs the outbound upstream connection): `mcp-session-id` and
/// `last-event-id` are transport-managed for the *inbound* relay/worker hop
/// and MUST be forwarded so the worker's rmcp service can route sessions, so
/// they are blocked only on the outbound side.
pub fn is_forward_blocked_mcp_header(name: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "cookie",
        "connection",
        "keep-alive",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "proxy-authenticate",
        "host",
        "content-length",
    ];
    BLOCKED.contains(&name.trim().to_ascii_lowercase().as_str())
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
            .filter(|(name, _)| !is_forward_blocked_mcp_header(name))
            .map(|(name, value)| {
                serde_json::json!({ "name": name, "value": value })
            })
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
    use super::{
        extract_mcp_request_metadata, is_forward_blocked_mcp_header, parse_resource_target,
    };

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

    #[test]
    fn raw_usage_metadata_strips_credentials_and_cookies() {
        let headers = vec![
            (
                "authorization".to_string(),
                "Bearer super-secret".to_string(),
            ),
            ("cookie".to_string(), "session=topsecret".to_string()),
            ("connection".to_string(), "keep-alive".to_string()),
            ("mcp-protocol-version".to_string(), "2026-07-28".to_string()),
            (
                "x-prompt-ferry-conversation-id".to_string(),
                "conv-1".to_string(),
            ),
            ("user-agent".to_string(), "opencode/1.0".to_string()),
        ];
        let metadata = extract_mcp_request_metadata(
            None,
            &headers,
            br#"{"method":"tools/call","params":{"name":"t"}}"#,
        );
        let raw = metadata.request_raw_json.unwrap();
        let logged: Vec<(String, String)> = raw["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["name"].as_str().unwrap().to_string(),
                    entry["value"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(!logged.iter().any(|(name, _)| name == "authorization"));
        assert!(!logged.iter().any(|(name, _)| name == "cookie"));
        assert!(!logged.iter().any(|(name, _)| name == "connection"));
        assert!(
            !serde_json::to_string(&raw)
                .unwrap()
                .contains("super-secret"),
            "raw usage payload must not contain the bearer token"
        );
        assert!(
            !serde_json::to_string(&raw).unwrap().contains("topsecret"),
            "raw usage payload must not contain the cookie value"
        );
        assert!(
            logged
                .iter()
                .any(|(name, _)| name == "mcp-protocol-version")
        );
        assert!(
            logged
                .iter()
                .any(|(name, _)| name == "x-prompt-ferry-conversation-id")
        );
        assert!(logged.iter().any(|(name, _)| name == "user-agent"));
    }

    #[test]
    fn blocked_header_names_are_case_insensitive() {
        assert!(is_forward_blocked_mcp_header("Authorization"));
        assert!(is_forward_blocked_mcp_header("COOKIE"));
        assert!(is_forward_blocked_mcp_header("Host"));
        assert!(!is_forward_blocked_mcp_header("Mcp-Param-X-Region"));
        assert!(!is_forward_blocked_mcp_header("user-agent"));
    }
}
