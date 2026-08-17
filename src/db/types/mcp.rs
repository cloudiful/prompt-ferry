use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
pub struct McpBearerToken {
    pub token: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct McpServer {
    pub server_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    #[schema(value_type = String, example = "passthrough_preferred")]
    pub aggregate_naming_mode: String,
    pub transport: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: serde_json::Value,
    pub env_json: serde_json::Value,
    #[schema(value_type = Vec<McpBearerToken>)]
    #[serde(rename = "bearer_tokens")]
    pub bearer_tokens_json: serde_json::Value,
    pub http_headers_json: serde_json::Value,
    pub tool_filter_mode: String,
    pub allowed_tools: serde_json::Value,
    pub disabled_tools: serde_json::Value,
    pub disabled_resources: serde_json::Value,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub timeout_ms: i32,
    pub lifecycle_policy: String,
    pub lifecycle_manual_protocol_version: Option<String>,
    pub lifecycle_learned_mode: Option<String>,
    pub lifecycle_learned_protocol_version: Option<String>,
    pub lifecycle_learned_for_updated_at: Option<DateTime<Utc>>,
    pub lifecycle_learned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl McpBearerToken {
    pub fn parse_array(value: &Value) -> Vec<McpBearerToken> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| {
                value
                    .as_str()
                    .map(|token| McpBearerToken {
                        token: token.to_string(),
                        enabled: true,
                    })
                    .or_else(|| {
                        value.as_object().and_then(|object| {
                            object.get("token").and_then(Value::as_str).map(|token| {
                                McpBearerToken {
                                    token: token.to_string(),
                                    enabled: object
                                        .get("enabled")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(true),
                                }
                            })
                        })
                    })
            })
            .map(|mut value| {
                value.token = value.token.trim().to_string();
                value
            })
            .filter(|value| !value.token.is_empty())
            .collect()
    }
}

impl McpServer {
    pub fn bearer_tokens(&self) -> Vec<McpBearerToken> {
        McpBearerToken::parse_array(&self.bearer_tokens_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_with_tokens(tokens: Value) -> McpServer {
        McpServer {
            server_id: uuid::Uuid::nil(),
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "test".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: Value::Array(Vec::new()),
            env_json: Value::Object(Default::default()),
            bearer_tokens_json: tokens,
            http_headers_json: Value::Object(Default::default()),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: Value::Array(Vec::new()),
            disabled_tools: Value::Array(Vec::new()),
            disabled_resources: Value::Array(Vec::new()),
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
    fn reserved_http_header_detects_reserved_names_case_insensitively() {
        for name in [
            "authorization",
            "Authorization",
            "AUTHORIZATION",
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "keep-alive",
            "te",
            "trailer",
            "upgrade",
            "proxy-authenticate",
            "mcp-session-id",
            "last-event-id",
        ] {
            assert!(
                reserved_http_header(&serde_json::json!({ name: "x" })).is_some(),
                "{name} must be reserved"
            );
        }
        assert_eq!(
            reserved_http_header(&serde_json::json!({ "x-custom": "v" })),
            None
        );
        assert_eq!(reserved_http_header(&serde_json::json!([])), None);
    }

    #[test]
    fn bearer_tokens_defaults_missing_enabled_to_enabled() {
        let server = server_with_tokens(serde_json::json!([
            "legacy",
            { "token": "enabled-object" },
            { "token": "disabled-object", "enabled": false },
            { "enabled": true },
            "  "
        ]));

        assert_eq!(
            server.bearer_tokens(),
            vec![
                McpBearerToken {
                    token: "legacy".to_string(),
                    enabled: true,
                },
                McpBearerToken {
                    token: "enabled-object".to_string(),
                    enabled: true,
                },
                McpBearerToken {
                    token: "disabled-object".to_string(),
                    enabled: false,
                },
            ]
        );
    }
}

/// HTTP headers that `http_headers_json` must never set: credentials,
/// hop-by-hop headers, and MCP/SSE transport-managed headers. They would
/// conflict with the auth header, rmcp's session handling, or the wire
/// protocol, and could otherwise be used to smuggle credentials upstream.
pub const RESERVED_MCP_HTTP_HEADERS: [&str; 14] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
    "proxy-authenticate",
    "mcp-session-id",
    "last-event-id",
];

/// Returns the first reserved header name found in `http_headers_json`, if any.
pub fn reserved_http_header(headers: &serde_json::Value) -> Option<String> {
    headers.as_object()?.keys().find_map(|name| {
        let lower = name.trim().to_ascii_lowercase();
        RESERVED_MCP_HTTP_HEADERS
            .contains(&lower.as_str())
            .then(|| name.clone())
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerInput {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub aggregate_naming_mode: String,
    pub transport: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: serde_json::Value,
    pub env_json: serde_json::Value,
    pub bearer_tokens_json: serde_json::Value,
    pub http_headers_json: serde_json::Value,
    pub tool_filter_mode: String,
    pub allowed_tools: serde_json::Value,
    pub disabled_tools: serde_json::Value,
    pub disabled_resources: serde_json::Value,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub timeout_ms: i32,
    pub lifecycle_policy: String,
    pub lifecycle_manual_protocol_version: Option<String>,
}
