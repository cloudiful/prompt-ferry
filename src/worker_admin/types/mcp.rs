use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub servers: Vec<McpServer>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

/// Admin-API representation of an MCP server. Direct stdio environment values
/// are deliberately omitted; a null value means the saved value is retained
/// when the server is edited without replacing it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpServer {
    pub server_id: Uuid,
    pub source_endpoint_id: Option<Uuid>,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    #[schema(example = "passthrough_preferred")]
    pub aggregate_naming_mode: String,
    pub transport: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Value,
    pub env_json: Value,
    #[schema(value_type = Vec<db::McpBearerToken>)]
    pub bearer_tokens: Vec<db::McpBearerToken>,
    pub http_headers_json: Value,
    pub tool_filter_mode: String,
    pub allowed_tools: Value,
    pub disabled_tools: Value,
    pub disabled_resources: Value,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub timeout_ms: i32,
    pub lifecycle_policy: String,
    pub lifecycle_manual_protocol_version: Option<String>,
    pub lifecycle_learned_mode: Option<String>,
    pub lifecycle_learned_protocol_version: Option<String>,
    pub lifecycle_learned_for_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub lifecycle_learned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<&db::McpServer> for McpServer {
    fn from(server: &db::McpServer) -> Self {
        Self {
            server_id: server.server_id,
            source_endpoint_id: server.source_endpoint_id,
            scope: server.scope.clone(),
            owner_user_id: server.owner_user_id,
            name: server.name.clone(),
            aggregate_naming_mode: server.aggregate_naming_mode.clone(),
            transport: server.transport.clone(),
            url: server.url.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env_json: public_env_json(&server.env_json),
            bearer_tokens: server.bearer_tokens(),
            http_headers_json: server.http_headers_json.clone(),
            tool_filter_mode: server.tool_filter_mode.clone(),
            allowed_tools: server.allowed_tools.clone(),
            disabled_tools: server.disabled_tools.clone(),
            disabled_resources: server.disabled_resources.clone(),
            daily_max_requests: server.daily_max_requests,
            monthly_max_requests: server.monthly_max_requests,
            enabled: server.enabled,
            timeout_ms: server.timeout_ms,
            lifecycle_policy: server.lifecycle_policy.clone(),
            lifecycle_manual_protocol_version: server.lifecycle_manual_protocol_version.clone(),
            lifecycle_learned_mode: server.lifecycle_learned_mode.clone(),
            lifecycle_learned_protocol_version: server.lifecycle_learned_protocol_version.clone(),
            lifecycle_learned_for_updated_at: server.lifecycle_learned_for_updated_at,
            lifecycle_learned_at: server.lifecycle_learned_at,
            created_at: server.created_at,
            updated_at: server.updated_at,
        }
    }
}

fn public_env_json(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return Value::Object(Default::default());
    };
    Value::Object(
        object
            .iter()
            .map(|(name, value)| {
                let public_value = value
                    .as_str()
                    .and_then(db::mcp_env_reference_name)
                    .map(|_| value.clone())
                    .unwrap_or(Value::Null);
                (name.clone(), public_value)
            })
            .collect(),
    )
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct McpServerRequest {
    pub scope: Option<String>,
    pub owner_user_id: Option<i64>,
    pub source_endpoint_id: Option<Uuid>,
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
        self.validate(state, None, None, user).await
    }

    pub async fn validate_for_update(
        &self,
        state: &AdminState,
        existing_server_id: Uuid,
        existing_source_endpoint_id: Option<Uuid>,
        user: &SessionUser,
    ) -> Result<(), Response> {
        self.validate(
            state,
            Some(existing_server_id),
            existing_source_endpoint_id,
            user,
        )
        .await
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
        let env_json = merge_env_json(
            self.env_json.unwrap_or_else(|| serde_json::json!({})),
            existing_server.map(|server| &server.env_json),
        );
        // Only `builtin_minimax` rows are tied to a source endpoint. For
        // http/stdio updates, omitting `source_endpoint_id` must clear the
        // existing binding so the managed row can be reconfigured without
        // dragging the previous endpoint linkage along.
        let source_endpoint_id = if self.transport == "builtin_minimax" {
            self.source_endpoint_id
                .or_else(|| existing_server.and_then(|server| server.source_endpoint_id))
        } else {
            None
        };
        db::McpServerInput {
            scope,
            owner_user_id,
            source_endpoint_id,
            name: self.name,
            aggregate_naming_mode: self
                .aggregate_naming_mode
                .unwrap_or_else(|| "passthrough_preferred".to_string()),
            transport: self.transport,
            url: self.url,
            command: self.command,
            args: self.args.unwrap_or_else(|| serde_json::json!([])),
            env_json,
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
        existing_source_endpoint_id: Option<Uuid>,
        user: &SessionUser,
    ) -> Result<(), Response> {
        validate_request_budget_limit(self.daily_max_requests, "daily_max_requests")
            .map_err(|response| *response)?;
        validate_request_budget_limit(self.monthly_max_requests, "monthly_max_requests")
            .map_err(|response| *response)?;
        if !matches!(
            self.transport.as_str(),
            "http" | "stdio" | "builtin_minimax"
        ) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_transport",
                "transport must be http, stdio, or builtin_minimax",
            ));
        }
        // For builtin_minimax the request may legitimately omit
        // `source_endpoint_id` and inherit the existing binding. The
        // effective value drives both the source-presence check and the
        // MiniMax endpoint validation below, so http/stdio updates that
        // clear the binding do not leak the old endpoint here.
        let effective_source_endpoint_id = if self.transport == "builtin_minimax" {
            self.source_endpoint_id.or(existing_source_endpoint_id)
        } else {
            None
        };
        if self.transport == "builtin_minimax" && effective_source_endpoint_id.is_none() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_source_endpoint",
                "builtin_minimax requires a source endpoint",
            ));
        }
        if self.transport == "builtin_minimax"
            && let Some(endpoint_id) = effective_source_endpoint_id
        {
            let endpoint = db::get_endpoint(&state.pool, endpoint_id)
                .await
                .map_err(|err| internal(state, err))?;
            let Some(endpoint) = endpoint else {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_source_endpoint",
                    "MiniMax source endpoint not found",
                ));
            };
            if endpoint.provider != db::EndpointProvider::Minimax {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_source_endpoint",
                    "MiniMax source endpoint is required",
                ));
            }
            if endpoint.scope != scope_for_request(self, user)
                || endpoint.owner_user_id != owner_for_request(self, user)
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_source_endpoint",
                    "MiniMax source endpoint scope does not match the MCP server",
                ));
            }
        } else if self.source_endpoint_id.is_some() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_source_endpoint",
                "source_endpoint_id is only valid for builtin_minimax",
            ));
        }
        if self.transport == "stdio" {
            if self
                .command
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_command",
                    "stdio command is required",
                ));
            }
            if self.args.as_ref().is_some_and(|args| {
                !args.is_array()
                    || args
                        .as_array()
                        .is_some_and(|values| values.iter().any(|value| !value.is_string()))
            }) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_args",
                    "stdio args must be a JSON array of strings",
                ));
            }
            if self
                .env_json
                .as_ref()
                .is_some_and(|env| !valid_stdio_env(env, true))
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_env",
                    "stdio env must be an object with string values or worker references",
                ));
            }
        }
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

fn scope_for_request(request: &McpServerRequest, user: &SessionUser) -> String {
    if user.is_admin {
        request.scope.as_deref().unwrap_or("admin").to_string()
    } else {
        "user".to_string()
    }
}

fn owner_for_request(request: &McpServerRequest, user: &SessionUser) -> Option<i64> {
    if user.is_admin {
        request.owner_user_id
    } else {
        Some(user.user_id)
    }
}

fn valid_stdio_env(value: &Value, allow_preserve_null: bool) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.iter().all(|(name, value)| {
        let valid_name = !name.is_empty()
            && name.bytes().enumerate().all(|(index, byte)| match index {
                0 => byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_lowercase(),
                _ => {
                    byte == b'_'
                        || byte.is_ascii_uppercase()
                        || byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                }
            });
        let valid_value = value.as_str().is_some_and(|value| {
            !value.starts_with("{env:") || db::mcp_env_reference_name(value).is_some()
        });
        valid_name && (valid_value || (allow_preserve_null && value.is_null()))
    })
}

fn merge_env_json(submitted: Value, existing: Option<&Value>) -> Value {
    let Some(submitted_object) = submitted.as_object() else {
        return submitted;
    };
    let existing_object = existing.and_then(Value::as_object);
    Value::Object(
        submitted_object
            .iter()
            .filter_map(|(name, value)| {
                if value.is_null() {
                    existing_object
                        .and_then(|object| object.get(name))
                        .cloned()
                        .map(|value| (name.clone(), value))
                } else {
                    Some((name.clone(), value.clone()))
                }
            })
            .collect(),
    )
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
    use super::{
        McpServerRequest, SessionUser, is_valid_protocol_version, merge_env_json, public_env_json,
    };
    use crate::db::McpServer;
    use uuid::Uuid;

    fn admin_user() -> SessionUser {
        SessionUser {
            user_id: 1,
            login_name: "admin".to_string(),
            display_name: "Admin".to_string(),
            is_admin: true,
        }
    }

    fn existing_with_source(endpoint_id: Uuid) -> McpServer {
        McpServer {
            server_id: Uuid::new_v4(),
            source_endpoint_id: Some(endpoint_id),
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "managed".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "builtin_minimax".to_string(),
            url: None,
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
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

    fn request_for_transport(transport: &str) -> McpServerRequest {
        McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "reconfigured".to_string(),
            aggregate_naming_mode: None,
            transport: transport.to_string(),
            url: if transport == "http" {
                Some("http://127.0.0.1:3000/mcp".to_string())
            } else {
                None
            },
            command: if transport == "stdio" {
                Some("mcpd".to_string())
            } else {
                None
            },
            args: None,
            env_json: None,
            bearer_tokens: None,
            http_headers_json: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
    }

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

    #[test]
    fn public_environment_values_are_hidden_but_worker_references_remain() {
        let public = public_env_json(&serde_json::json!({
            "MINIMAX_API_KEY": "secret",
            "MINIMAX_API_HOST": "{env:MINIMAX_API_HOST}",
        }));

        assert_eq!(
            public,
            serde_json::json!({
                "MINIMAX_API_KEY": null,
                "MINIMAX_API_HOST": "{env:MINIMAX_API_HOST}"
            })
        );
    }

    #[test]
    fn null_environment_values_preserve_existing_values_on_update() {
        let merged = merge_env_json(
            serde_json::json!({ "MINIMAX_API_KEY": null, "NEW_VALUE": "new" }),
            Some(&serde_json::json!({ "MINIMAX_API_KEY": "secret" })),
        );

        assert_eq!(
            merged,
            serde_json::json!({
                "MINIMAX_API_KEY": "secret",
                "NEW_VALUE": "new"
            })
        );
    }

    #[test]
    fn managed_binding_preserved_for_builtin_minimax_update() {
        let endpoint_id = Uuid::new_v4();
        let existing = existing_with_source(endpoint_id);
        let request = McpServerRequest {
            source_endpoint_id: None,
            ..request_for_transport("builtin_minimax")
        };

        let input = request.into_input(&admin_user(), Some(&existing));
        assert_eq!(input.source_endpoint_id, Some(endpoint_id));
    }

    #[test]
    fn managed_binding_cleared_for_http_update() {
        let endpoint_id = Uuid::new_v4();
        let existing = existing_with_source(endpoint_id);
        let request = McpServerRequest {
            source_endpoint_id: None,
            ..request_for_transport("http")
        };

        let input = request.into_input(&admin_user(), Some(&existing));
        assert_eq!(input.source_endpoint_id, None);
    }

    #[test]
    fn managed_binding_cleared_for_stdio_update() {
        let endpoint_id = Uuid::new_v4();
        let existing = existing_with_source(endpoint_id);
        let request = McpServerRequest {
            source_endpoint_id: None,
            ..request_for_transport("stdio")
        };

        let input = request.into_input(&admin_user(), Some(&existing));
        assert_eq!(input.source_endpoint_id, None);
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
