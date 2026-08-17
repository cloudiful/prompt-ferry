use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    db,
    worker_admin_state::{AdminState, error, internal},
};
use axum::{http::StatusCode, response::Response};

use super::{SessionUser, validate_request_budget_limit};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpServerPageResponse {
    pub servers: Vec<db::McpServer>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct McpServerRequest {
    pub scope: Option<String>,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub aggregate_naming_mode: Option<String>,
    pub transport: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Option<serde_json::Value>,
    pub env_json: Option<serde_json::Value>,
    pub bearer_tokens: Option<Vec<db::McpBearerToken>>,
    pub http_headers_json: Option<serde_json::Value>,
    pub tool_filter_mode: Option<String>,
    pub allowed_tools: Option<serde_json::Value>,
    pub disabled_tools: Option<serde_json::Value>,
    pub disabled_resources: Option<serde_json::Value>,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: Option<bool>,
    pub timeout_ms: Option<i32>,
    pub lifecycle_policy: Option<String>,
    pub lifecycle_manual_protocol_version: Option<String>,
}

impl McpServerRequest {
    pub async fn validate_for_create(
        &self,
        state: &AdminState,
        user: &SessionUser,
    ) -> Result<(), Response> {
        self.validate(state, None, user).await
    }

    pub async fn validate_for_update(
        &self,
        state: &AdminState,
        existing_server_id: Uuid,
        user: &SessionUser,
    ) -> Result<(), Response> {
        self.validate(state, Some(existing_server_id), user).await
    }

    pub fn into_input(
        self,
        user: &SessionUser,
        existing_server: Option<&db::McpServer>,
    ) -> db::McpServerInput {
        let (scope, owner_user_id) = if user.is_admin {
            (
                self.scope.unwrap_or_else(|| "admin".to_string()),
                self.owner_user_id,
            )
        } else {
            ("user".to_string(), Some(user.user_id))
        };
        db::McpServerInput {
            scope,
            owner_user_id,
            name: self.name,
            aggregate_naming_mode: self
                .aggregate_naming_mode
                .unwrap_or_else(|| "passthrough_preferred".to_string()),
            transport: self.transport,
            url: self.url,
            command: self.command,
            args: self.args.unwrap_or_else(|| serde_json::json!([])),
            env_json: self.env_json.unwrap_or_else(|| serde_json::json!({})),
            bearer_tokens_json: self
                .bearer_tokens
                .map(|tokens| {
                    serde_json::Value::Array(
                        tokens
                            .into_iter()
                            .map(|mut value| {
                                value.token = value.token.trim().to_string();
                                value
                            })
                            .filter(|value| !value.token.is_empty())
                            .map(|value| {
                                serde_json::json!({
                                    "token": value.token,
                                    "enabled": value.enabled,
                                })
                            })
                            .collect(),
                    )
                })
                .or_else(|| existing_server.map(|server| server.bearer_tokens_json.clone()))
                .unwrap_or_else(|| serde_json::json!([])),
            http_headers_json: self
                .http_headers_json
                .unwrap_or_else(|| serde_json::json!({})),
            tool_filter_mode: self
                .tool_filter_mode
                .unwrap_or_else(|| "blacklist".to_string()),
            allowed_tools: self.allowed_tools.unwrap_or_else(|| serde_json::json!([])),
            disabled_tools: self.disabled_tools.unwrap_or_else(|| serde_json::json!([])),
            disabled_resources: self
                .disabled_resources
                .unwrap_or_else(|| serde_json::json!([])),
            daily_max_requests: self.daily_max_requests,
            monthly_max_requests: self.monthly_max_requests,
            enabled: self.enabled.unwrap_or(true),
            timeout_ms: self.timeout_ms.unwrap_or(30_000).clamp(100, 300_000),
            lifecycle_policy: self.lifecycle_policy.unwrap_or_else(|| {
                existing_server
                    .map(|server| server.lifecycle_policy.clone())
                    .unwrap_or_else(|| "auto".to_string())
            }),
            lifecycle_manual_protocol_version: match self.lifecycle_manual_protocol_version {
                Some(value) => {
                    let value = value.trim().to_string();
                    if value.is_empty() { None } else { Some(value) }
                }
                None => existing_server
                    .and_then(|server| server.lifecycle_manual_protocol_version.clone()),
            },
        }
    }

    async fn validate(
        &self,
        state: &AdminState,
        existing_server_id: Option<Uuid>,
        user: &SessionUser,
    ) -> Result<(), Response> {
        validate_request_budget_limit(self.daily_max_requests, "daily_max_requests")
            .map_err(|response| *response)?;
        validate_request_budget_limit(self.monthly_max_requests, "monthly_max_requests")
            .map_err(|response| *response)?;
        let name = self.name.trim();
        if name.is_empty() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_name",
                "mcp server name is required",
            ));
        }
        let scope = if user.is_admin {
            self.scope.as_deref().unwrap_or("admin")
        } else {
            "user"
        };
        if !matches!(scope, "admin" | "user") {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                "scope must be admin or user",
            ));
        }
        if let Some(tool_filter_mode) = self.tool_filter_mode.as_deref()
            && !matches!(tool_filter_mode, "blacklist" | "whitelist")
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_tool_filter_mode",
                "tool_filter_mode must be blacklist or whitelist",
            ));
        }
        if let Some(lifecycle_policy) = self.lifecycle_policy.as_deref()
            && !matches!(lifecycle_policy, "auto" | "legacy_initialize")
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_lifecycle_policy",
                "lifecycle_policy must be auto or legacy_initialize",
            ));
        }
        if let Some(version) = self.lifecycle_manual_protocol_version.as_deref()
            && !version.trim().is_empty()
            && !is_valid_protocol_version(version.trim())
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_lifecycle_protocol_version",
                "lifecycle_manual_protocol_version must be a protocol version date such as 2025-06-18",
            ));
        }
        if let Some(aggregate_naming_mode) = self.aggregate_naming_mode.as_deref()
            && !matches!(
                aggregate_naming_mode,
                "qualified_only" | "passthrough_preferred"
            )
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_aggregate_naming_mode",
                "aggregate_naming_mode must be qualified_only or passthrough_preferred",
            ));
        }
        if let Some(tokens) = &self.bearer_tokens {
            if tokens.iter().any(|value| value.token.trim().is_empty()) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_bearer_tokens",
                    "bearer_tokens must not contain empty values",
                ));
            }
            if !tokens.is_empty() && !tokens.iter().any(|value| value.enabled) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_bearer_tokens",
                    "at least one bearer token must be enabled",
                ));
            }
        }
        if let Some(http_headers) = &self.http_headers_json
            && let Some(name) = db::reserved_http_header(http_headers)
        {
            let message = format!("http_headers_json must not override reserved header `{name}`");
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_http_headers",
                &message,
            ));
        }
        let owner_user_id = if user.is_admin {
            self.owner_user_id
        } else {
            Some(user.user_id)
        };
        if scope == "admin" && owner_user_id.is_some() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "admin mcp server cannot have owner",
            ));
        }
        if scope == "user" && owner_user_id.is_none() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "user mcp server requires owner",
            ));
        }
        if let Some(owner_user_id) = owner_user_id {
            let owner = db::get_active_user(&state.pool, owner_user_id)
                .await
                .map_err(|err| internal(state, err))?;
            if owner.is_none() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_owner",
                    "owner user not found or inactive",
                ));
            }
        }
        let duplicate = db::get_mcp_server_by_name(&state.pool, name)
            .await
            .map_err(|err| internal(state, err))?;
        if duplicate.is_some_and(|server| Some(server.server_id) != existing_server_id) {
            return Err(error(
                StatusCode::CONFLICT,
                "duplicate_mcp_server",
                "mcp server name already exists",
            ));
        }
        Ok(())
    }
}

/// A plausible MCP protocol-version date (`YYYY-MM-DD`). Month and day ranges
/// are validated so an operator typo is rejected at save time instead of being
/// silently ignored at connect time.
fn is_valid_protocol_version(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !value[0..4].chars().all(|c| c.is_ascii_digit())
        || !value[5..7].chars().all(|c| c.is_ascii_digit())
        || !value[8..10].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..10].parse::<u32>().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::is_valid_protocol_version;

    #[test]
    fn protocol_version_validation_accepts_known_dates() {
        assert!(is_valid_protocol_version("2026-07-28"));
        assert!(is_valid_protocol_version("2025-06-18"));
        assert!(is_valid_protocol_version("2024-10-07"));
    }

    #[test]
    fn protocol_version_validation_rejects_garbage() {
        assert!(!is_valid_protocol_version("2025-13-01"));
        assert!(!is_valid_protocol_version("2025-06-32"));
        assert!(!is_valid_protocol_version("06-18"));
        assert!(!is_valid_protocol_version("latest"));
        assert!(!is_valid_protocol_version(""));
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpTestResponse {
    pub ok: bool,
    pub message: String,
    #[schema(value_type = u64)]
    pub duration_ms: u128,
    pub tool_count: usize,
    pub resource_count: usize,
    pub prompt_count: usize,
    pub tools: Vec<McpCatalogItem>,
    pub resources: Vec<McpCatalogItem>,
    pub prompts: Vec<McpCatalogItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpCatalogResponse {
    pub tools: Vec<McpCatalogItem>,
    pub resources: Vec<McpCatalogItem>,
    pub prompts: Vec<McpCatalogItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpCatalogItem {
    pub name: String,
    pub aggregate_names: Vec<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}
