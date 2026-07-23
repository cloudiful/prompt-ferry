use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
    #[schema(value_type = Vec<String>)]
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl McpServer {
    pub fn bearer_tokens(&self) -> Vec<String> {
        self.bearer_tokens_json
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(str::trim))
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()
    }
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
}
