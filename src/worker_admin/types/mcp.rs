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
    /// Provider preset id; `null` means the untyped legacy generic behavior.
    pub provider_kind: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Value,
    pub env_json: Value,
    #[schema(value_type = Vec<db::McpBearerToken>)]
    pub bearer_tokens: Vec<db::McpBearerToken>,
    pub http_headers_json: Value,
    pub auth_mode: String,
    pub basic_username: Option<String>,
    pub has_basic_password: bool,
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
            provider_kind: server.provider_kind.clone(),
            url: server.url.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env_json: public_env_json(&server.env_json),
            bearer_tokens: server.bearer_tokens(),
            http_headers_json: server.http_headers_json.clone(),
            auth_mode: server.effective_auth_mode().to_string(),
            basic_username: server.basic_username.clone(),
            has_basic_password: server.has_basic_password(),
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
    /// Provider preset id. `null`/omitted keeps the existing (or untyped
    /// legacy) value; `"generic"` explicitly clears a preset binding.
    pub provider_kind: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Option<serde_json::Value>,
    pub env_json: Option<serde_json::Value>,
    pub bearer_tokens: Option<Vec<db::McpBearerToken>>,
    pub http_headers_json: Option<serde_json::Value>,
    pub auth_mode: Option<String>,
    pub basic_username: Option<String>,
    pub basic_password: Option<String>,
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
        // Explicit auth mode so bearer/basic remain unambiguous. Preserve
        // legacy bearer configs: if the request omits auth_mode, keep the
        // existing effective mode or infer from provided credentials.
        let mut auth_mode =
            self.auth_mode
                .as_deref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    if let Some(existing) = existing_server {
                        existing.effective_auth_mode().to_string()
                    } else {
                        let has_bearer = self.bearer_tokens.as_ref().is_some_and(|tokens| {
                            tokens.iter().any(|t| !t.token.trim().is_empty())
                        });
                        let has_basic = self
                            .basic_username
                            .as_deref()
                            .is_some_and(|v| !v.trim().is_empty())
                            || self
                                .basic_password
                                .as_deref()
                                .is_some_and(|v| !v.trim().is_empty());
                        if has_bearer {
                            db::MCP_AUTH_MODE_BEARER.to_string()
                        } else if has_basic {
                            db::MCP_AUTH_MODE_BASIC.to_string()
                        } else {
                            db::MCP_AUTH_MODE_NONE.to_string()
                        }
                    }
                });
        // Non-http transports never carry HTTP auth.
        if self.transport != "http" {
            auth_mode = db::MCP_AUTH_MODE_NONE.to_string();
        } else if !db::is_valid_auth_mode(&auth_mode) {
            auth_mode = db::MCP_AUTH_MODE_NONE.to_string();
        }
        let bearer_tokens_json = self
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
            .unwrap_or_else(|| serde_json::json!([]));
        // Basic credentials: keep existing when the request omits them so a
        // bearer<->basic switch does not wipe the other credential set.
        let basic_username = match self.basic_username {
            Some(value) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    existing_server.and_then(|s| s.basic_username.clone())
                } else {
                    Some(trimmed)
                }
            }
            None => existing_server.and_then(|s| s.basic_username.clone()),
        };
        let basic_password = match self.basic_password {
            Some(value) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    // Empty string means keep existing when updating, or no
                    // password when creating.
                    existing_server.and_then(|s| s.basic_password.clone())
                } else {
                    Some(trimmed)
                }
            }
            None => existing_server.and_then(|s| s.basic_password.clone()),
        };
        // When auth_mode is not basic, do not clear stored basic credentials
        // so they remain for a later switch. Same for bearer when not selected.
        let (basic_username, basic_password) = if auth_mode == db::MCP_AUTH_MODE_BASIC {
            (basic_username, basic_password)
        } else {
            // Preserve existing basic credentials even when not active.
            (
                existing_server
                    .and_then(|s| s.basic_username.clone())
                    .or(basic_username),
                existing_server
                    .and_then(|s| s.basic_password.clone())
                    .or(basic_password),
            )
        };
        // Managed rows (builtin_minimax) are always the minimax preset;
        // explicit requests cannot re-label them. Presets only ride on the
        // standard http transport, so switching a row to stdio drops any
        // previously attached preset.
        let provider_kind = if self.transport == "builtin_minimax" {
            Some(db::MCP_PROVIDER_MINIMAX.to_string())
        } else if self.transport != "http" {
            None
        } else {
            match self.provider_kind.as_deref().map(str::trim) {
                // Field omitted: keep the stored value (or untyped for new
                // rows) so legacy clients are unaffected.
                None => existing_server.and_then(|server| server.provider_kind.clone()),
                // Explicit empty string or "generic" clears the binding.
                Some("") | Some(db::MCP_PROVIDER_GENERIC) => None,
                Some(value) => Some(value.to_string()),
            }
        };
        // Hosted presets are server-derived: a Context7/Firecrawl row always
        // uses the official endpoint and bearer auth regardless of what the
        // client omitted, so a nullable request can never persist a preset
        // with a contradictory URL or auth mode.
        let hosted_preset = hosted_bearer_preset(&self.transport, provider_kind.as_deref());
        if hosted_preset.is_some() {
            auth_mode = db::MCP_AUTH_MODE_BEARER.to_string();
        }
        db::McpServerInput {
            scope,
            owner_user_id,
            source_endpoint_id,
            name: self.name,
            aggregate_naming_mode: self
                .aggregate_naming_mode
                .unwrap_or_else(|| "passthrough_preferred".to_string()),
            transport: self.transport,
            provider_kind,
            url: match hosted_preset {
                Some(info) => info.default_url.map(str::to_string),
                None => self.url,
            },
            command: self.command,
            args: self.args.unwrap_or_else(|| serde_json::json!([])),
            env_json,
            bearer_tokens_json,
            http_headers_json: self
                .http_headers_json
                .unwrap_or_else(|| serde_json::json!({})),
            auth_mode,
            basic_username,
            basic_password,
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
        // Provider preset validation (issue #296 Phase 1). `generic` is the
        // implicit untyped value and also an explicit "clear preset" signal,
        // so it carries no transport constraint. Hosted presets require the
        // standard http transport; the managed MiniMax projection stays on
        // builtin_minimax and keeps its source_endpoint_id binding. Unknown
        // ids are rejected instead of being stored.
        if let Some(provider_kind) = self.provider_kind.as_deref().map(str::trim) {
            if !provider_kind.is_empty() && provider_kind != db::MCP_PROVIDER_GENERIC {
                if !db::is_known_mcp_provider(provider_kind) {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        "invalid_provider_kind",
                        "provider_kind must be one of generic, minimax, context7, firecrawl",
                    ));
                }
                if provider_kind == db::MCP_PROVIDER_MINIMAX {
                    if self.transport != "builtin_minimax" {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_provider_kind",
                            "provider_kind minimax requires the builtin_minimax transport",
                        ));
                    }
                } else if self.transport != "http" {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        "invalid_provider_kind",
                        "provider_kind presets require the http transport",
                    ));
                }
            }
        }
        // Hosted preset consistency (issue #296 Phase 2): explicit or
        // inherited Context7/Firecrawl rows must use the official endpoint and
        // bearer auth. Custom/self-hosted endpoints must select generic mode
        // explicitly so a row never mixes a preset id with a contradictory URL
        // or auth style. When the client omits provider_kind on update, the
        // inherited preset still drives the check.
        if self.transport == "http" {
            let provider_kind = match self.provider_kind.as_deref().map(str::trim) {
                Some("") | Some(db::MCP_PROVIDER_GENERIC) => None,
                Some(value) if !value.is_empty() => Some(value.to_string()),
                _ => match existing_server_id {
                    Some(id) => state
                        .config_repository
                        .get_mcp_server(id)
                        .await
                        .map_err(|err| internal(state, err))?
                        .map(|server| server.effective_provider_kind().to_string()),
                    None => None,
                },
            };
            if let Some(info) = hosted_bearer_preset("http", provider_kind.as_deref()) {
                let default_url = info.default_url.unwrap_or_default();
                if let Some(url) = self
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    && !urls_equivalent(url, default_url)
                {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        "invalid_provider_url",
                        &format!(
                            "{} preset uses the official endpoint {default_url}; choose generic for a custom URL",
                            info.id
                        ),
                    ));
                }
                if let Some(auth_mode) = self
                    .auth_mode
                    .as_deref()
                    .map(str::trim)
                    .filter(|mode| !mode.is_empty())
                    && auth_mode != db::MCP_AUTH_MODE_BEARER
                {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        "invalid_provider_auth",
                        &format!("{} preset only supports bearer auth", info.id),
                    ));
                }
                let has_usable_request_token = self.bearer_tokens.as_ref().is_some_and(|tokens| {
                    tokens
                        .iter()
                        .any(|token| token.enabled && !token.token.trim().is_empty())
                });
                if !has_usable_request_token {
                    // An explicit `bearer_tokens: []` means "clear the
                    // credentials": a hosted preset must never be persisted
                    // with an empty token merely because the previous row had
                    // one. Only an omitted field inherits the existing token.
                    if self.bearer_tokens.is_some() {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_bearer_tokens",
                            &format!("{} preset requires a bearer token", info.id),
                        ));
                    }
                    let has_existing = match existing_server_id {
                        Some(id) => state
                            .config_repository
                            .get_mcp_server(id)
                            .await
                            .map_err(|err| internal(state, err))?
                            .is_some_and(|server| {
                                server
                                    .bearer_tokens()
                                    .iter()
                                    .any(|token| token.enabled && !token.token.trim().is_empty())
                            }),
                        None => false,
                    };
                    if !has_existing {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_bearer_tokens",
                            &format!("{} preset requires a bearer token", info.id),
                        ));
                    }
                }
            }
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
            let endpoint = state
                .config_repository
                .get_endpoint(endpoint_id)
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
        if let Some(auth_mode) = self.auth_mode.as_deref() {
            let trimmed = auth_mode.trim();
            if !trimmed.is_empty() && !db::is_valid_auth_mode(trimmed) {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_auth_mode",
                    "auth_mode must be none, bearer, or basic",
                ));
            }
            if self.transport != "http" && !trimmed.is_empty() && trimmed != db::MCP_AUTH_MODE_NONE
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_auth_mode",
                    "only http transport supports bearer or basic auth",
                ));
            }
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
        // HTTP auth mode validation
        if self.transport == "http" {
            let requested_auth = self.auth_mode.as_deref().map(|v| v.trim()).unwrap_or("");
            if requested_auth == db::MCP_AUTH_MODE_BEARER {
                let has_request_tokens = self
                    .bearer_tokens
                    .as_ref()
                    .is_some_and(|tokens| tokens.iter().any(|t| !t.token.trim().is_empty()));
                if !has_request_tokens {
                    // Need existing tokens when updating
                    let has_existing = if let Some(id) = existing_server_id {
                        state
                            .config_repository
                            .get_mcp_server(id)
                            .await
                            .map_err(|err| internal(state, err))?
                            .is_some_and(|server| !server.bearer_tokens().is_empty())
                    } else {
                        false
                    };
                    if !has_existing {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_bearer_tokens",
                            "bearer auth requires at least one bearer token",
                        ));
                    }
                }
            }
            if requested_auth == db::MCP_AUTH_MODE_BASIC {
                let username = self.basic_username.as_deref().unwrap_or("").trim();
                let password = self.basic_password.as_deref().unwrap_or("").trim();
                if username.is_empty() {
                    // Allow keeping existing username on update
                    let has_existing = if let Some(id) = existing_server_id {
                        state
                            .config_repository
                            .get_mcp_server(id)
                            .await
                            .map_err(|err| internal(state, err))?
                            .and_then(|s| s.basic_username)
                            .is_some_and(|v| !v.trim().is_empty())
                    } else {
                        false
                    };
                    if !has_existing {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_basic_auth",
                            "basic auth requires a username",
                        ));
                    }
                }
                if password.is_empty() {
                    let has_existing = if let Some(id) = existing_server_id {
                        state
                            .config_repository
                            .get_mcp_server(id)
                            .await
                            .map_err(|err| internal(state, err))?
                            .and_then(|s| s.basic_password)
                            .is_some_and(|v| !v.trim().is_empty())
                    } else {
                        false
                    };
                    if !has_existing {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "invalid_basic_auth",
                            "basic auth requires a password",
                        ));
                    }
                }
            }
        } else {
            // Non-http transports must not carry HTTP auth material
            if self
                .basic_username
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty())
                || self
                    .basic_password
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty())
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_auth_mode",
                    "only http transport supports basic auth",
                ));
            }
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
            let owner = state
                .user_store
                .get_active_user(owner_user_id)
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
        let duplicate = state
            .config_repository
            .get_mcp_server_by_name(name)
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

/// Registry metadata for a hosted HTTP preset that authenticates with a bearer
/// token. `None` for generic rows, the managed MiniMax projection, stdio, and
/// unknown values, so those paths keep their existing behavior.
fn hosted_bearer_preset(
    transport: &str,
    provider_kind: Option<&str>,
) -> Option<&'static db::McpProviderInfo> {
    if transport != "http" {
        return None;
    }
    let info = db::mcp_provider_info(provider_kind)?;
    (info.default_url.is_some() && info.auth == db::McpProviderAuth::Bearer).then_some(info)
}

/// Preset URLs are canonical, so a single trailing slash is tolerated while
/// any other difference means the row is a custom endpoint (generic mode).
fn urls_equivalent(left: &str, right: &str) -> bool {
    left.trim().trim_end_matches('/') == right.trim().trim_end_matches('/')
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
        super::tests::{test_state, test_state_with_pool_url},
        McpServerRequest, SessionUser, is_valid_protocol_version, merge_env_json, public_env_json,
    };
    use crate::db::McpServer;
    use axum::http::StatusCode;
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
            provider_kind: Some(crate::db::MCP_PROVIDER_MINIMAX.to_string()),
            url: None,
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            auth_mode: crate::db::MCP_AUTH_MODE_NONE.to_string(),
            basic_username: None,
            basic_password: None,
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
            provider_kind: None,
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
            auth_mode: None,
            basic_username: None,
            basic_password: None,
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

    #[test]
    fn managed_minimax_rows_always_carry_the_minimax_preset() {
        let endpoint_id = Uuid::new_v4();
        let existing = existing_with_source(endpoint_id);
        let request = McpServerRequest {
            source_endpoint_id: None,
            provider_kind: Some("context7".to_string()),
            ..request_for_transport("builtin_minimax")
        };

        let input = request.into_input(&admin_user(), Some(&existing));
        assert_eq!(
            input.provider_kind.as_deref(),
            Some(crate::db::MCP_PROVIDER_MINIMAX),
            "explicit preset must not relabel a managed minimax row"
        );
    }

    #[test]
    fn response_provider_kind_preserves_stored_value() {
        let managed = existing_with_source(Uuid::new_v4());
        let response: super::McpServer = (&managed).into();
        assert_eq!(response.provider_kind.as_deref(), Some("minimax"));

        let mut legacy = existing_with_source(Uuid::new_v4());
        legacy.transport = "http".to_string();
        legacy.provider_kind = None;
        let response: super::McpServer = (&legacy).into();
        assert_eq!(
            response.provider_kind, None,
            "legacy rows keep the untyped null value"
        );
    }

    #[test]
    fn provider_kind_omitted_keeps_existing_value() {
        let mut existing = existing_with_source(Uuid::new_v4());
        existing.transport = "http".to_string();
        existing.provider_kind = Some("firecrawl".to_string());

        let input = McpServerRequest {
            ..request_for_transport("http")
        }
        .into_input(&admin_user(), Some(&existing));
        assert_eq!(input.provider_kind.as_deref(), Some("firecrawl"));

        let created = McpServerRequest {
            ..request_for_transport("http")
        }
        .into_input(&admin_user(), None);
        assert_eq!(created.provider_kind, None);
    }

    #[test]
    fn provider_kind_empty_or_generic_clears_preset() {
        let mut existing = existing_with_source(Uuid::new_v4());
        existing.transport = "http".to_string();
        existing.provider_kind = Some("context7".to_string());

        for cleared in ["", "generic", "  "] {
            let input = McpServerRequest {
                provider_kind: Some(cleared.to_string()),
                ..request_for_transport("http")
            }
            .into_input(&admin_user(), Some(&existing));
            assert_eq!(input.provider_kind, None, "value {cleared:?} must clear");
        }
    }

    #[test]
    fn provider_kind_is_dropped_when_transport_switches_to_stdio() {
        let mut existing = existing_with_source(Uuid::new_v4());
        existing.transport = "http".to_string();
        existing.provider_kind = Some("firecrawl".to_string());

        let input = McpServerRequest {
            command: Some("mcpd".to_string()),
            ..request_for_transport("stdio")
        }
        .into_input(&admin_user(), Some(&existing));
        assert_eq!(input.provider_kind, None);
    }

    #[test]
    fn provider_kind_explicit_preset_is_applied_verbatim() {
        let input = McpServerRequest {
            provider_kind: Some("  context7  ".to_string()),
            ..request_for_transport("http")
        }
        .into_input(&admin_user(), None);
        assert_eq!(input.provider_kind.as_deref(), Some("context7"));
    }

    #[tokio::test]
    async fn provider_kind_rejects_unknown_values() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            name: "provider-kind-unknown".to_string(),
            provider_kind: Some("unknown-provider".to_string()),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn provider_kind_requires_http_transport() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            command: Some("mcpd".to_string()),
            provider_kind: Some("context7".to_string()),
            ..request_for_transport("stdio")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn provider_kind_minimax_requires_builtin_transport() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            provider_kind: Some("minimax".to_string()),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn provider_kind_accepts_context7_and_firecrawl_on_http() {
        // Positive validation needs the duplicate-name lookup, so this test
        // runs against the shared dev database and skips when it is absent
        // (matching the DB-gated convention in `mcp::entry::tests`).
        let Ok(url) = std::env::var("PROMPT_FERRY_TEST_DATABASE_URL") else {
            eprintln!("skipping preset validation test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
            return;
        };
        let user = admin_user();
        for (value, preset_url) in [
            ("context7", "https://mcp.context7.com/mcp"),
            ("firecrawl", "https://mcp.firecrawl.dev/v2/mcp"),
        ] {
            let state = test_state_with_pool_url(&url);
            let request = McpServerRequest {
                name: format!("preset-{value}"),
                provider_kind: Some(value.to_string()),
                url: Some(preset_url.to_string()),
                auth_mode: Some(crate::db::MCP_AUTH_MODE_BEARER.to_string()),
                bearer_tokens: Some(vec![crate::db::McpBearerToken {
                    token: "preset-token".to_string(),
                    enabled: true,
                }]),
                ..request_for_transport("http")
            };
            let result = request.validate_for_create(&state, &user).await;
            if let Err(err) = &result {
                eprintln!("preset {value} rejected: {}", err.status());
            }
            assert!(result.is_ok(), "preset {value} must validate");
        }
    }

    #[test]
    fn hosted_preset_matches_only_bearer_http_presets() {
        assert!(super::hosted_bearer_preset("http", Some("context7")).is_some());
        assert!(super::hosted_bearer_preset("http", Some("firecrawl")).is_some());
        assert!(super::hosted_bearer_preset("http", Some("generic")).is_none());
        assert!(super::hosted_bearer_preset("http", Some("minimax")).is_none());
        assert!(super::hosted_bearer_preset("http", None).is_none());
        assert!(super::hosted_bearer_preset("stdio", Some("context7")).is_none());
        assert!(super::hosted_bearer_preset("builtin_minimax", Some("minimax")).is_none());
    }

    #[test]
    fn preset_url_equivalence_tolerates_trailing_slash_only() {
        assert!(super::urls_equivalent(
            "https://mcp.context7.com/mcp/",
            "https://mcp.context7.com/mcp"
        ));
        assert!(!super::urls_equivalent(
            "https://self-hosted.example.com/mcp",
            "https://mcp.context7.com/mcp"
        ));
    }

    #[test]
    fn preset_into_input_derives_default_url_and_bearer_auth() {
        for (value, preset_url) in [
            ("context7", "https://mcp.context7.com/mcp"),
            ("firecrawl", "https://mcp.firecrawl.dev/v2/mcp"),
        ] {
            let input = McpServerRequest {
                provider_kind: Some(value.to_string()),
                url: None,
                auth_mode: None,
                bearer_tokens: Some(vec![crate::db::McpBearerToken {
                    token: "preset-token".to_string(),
                    enabled: true,
                }]),
                ..request_for_transport("http")
            }
            .into_input(&admin_user(), None);

            assert_eq!(input.provider_kind.as_deref(), Some(value));
            assert_eq!(
                input.url.as_deref(),
                Some(preset_url),
                "{value} must persist the official endpoint"
            );
            assert_eq!(input.auth_mode, crate::db::MCP_AUTH_MODE_BEARER);
        }
    }

    #[test]
    fn preset_into_input_keeps_preset_when_request_omits_provider_kind() {
        let mut existing = existing_with_source(Uuid::new_v4());
        existing.transport = "http".to_string();
        existing.provider_kind = Some("context7".to_string());
        existing.url = Some("https://mcp.context7.com/mcp".to_string());
        existing.auth_mode = crate::db::MCP_AUTH_MODE_BEARER.to_string();
        existing.bearer_tokens_json = serde_json::json!(["stored-token"]);

        let input = McpServerRequest {
            url: Some("https://legacy.example.com/mcp".to_string()),
            auth_mode: None,
            ..request_for_transport("http")
        }
        .into_input(&admin_user(), Some(&existing));

        assert_eq!(input.provider_kind.as_deref(), Some("context7"));
        assert_eq!(input.url.as_deref(), Some("https://mcp.context7.com/mcp"));
        assert_eq!(input.auth_mode, crate::db::MCP_AUTH_MODE_BEARER);
        assert_eq!(
            input.bearer_tokens_json,
            serde_json::json!(["stored-token"])
        );
    }

    #[test]
    fn generic_custom_url_into_input_is_untouched() {
        let input = McpServerRequest {
            provider_kind: Some("generic".to_string()),
            url: Some("https://self-hosted.example.com/mcp".to_string()),
            ..request_for_transport("http")
        }
        .into_input(&admin_user(), None);

        assert_eq!(input.provider_kind, None);
        assert_eq!(
            input.url.as_deref(),
            Some("https://self-hosted.example.com/mcp")
        );
    }

    #[tokio::test]
    async fn preset_rejects_custom_url_before_persistence() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            name: "preset-custom-url".to_string(),
            provider_kind: Some("context7".to_string()),
            url: Some("https://self-hosted.example.com/mcp".to_string()),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn preset_rejects_non_bearer_auth() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            name: "preset-basic-auth".to_string(),
            provider_kind: Some("firecrawl".to_string()),
            url: Some("https://mcp.firecrawl.dev/v2/mcp".to_string()),
            auth_mode: Some(crate::db::MCP_AUTH_MODE_BASIC.to_string()),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn preset_requires_a_bearer_token() {
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            name: "preset-no-token".to_string(),
            provider_kind: Some("context7".to_string()),
            url: Some("https://mcp.context7.com/mcp".to_string()),
            auth_mode: Some(crate::db::MCP_AUTH_MODE_BEARER.to_string()),
            bearer_tokens: Some(vec![]),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_create(&state, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn preset_update_rejects_explicitly_cleared_bearer_tokens() {
        // Reviewer P3: an explicit `bearer_tokens: []` on update is a request
        // to clear the credentials and must not persist a hosted preset with
        // no token just because the previous row had one. This returns before
        // any database lookup, so it runs without a live database.
        let state = test_state();
        let user = admin_user();
        let request = McpServerRequest {
            name: "preset-clear-token".to_string(),
            provider_kind: Some("firecrawl".to_string()),
            url: Some("https://mcp.firecrawl.dev/v2/mcp".to_string()),
            auth_mode: Some(crate::db::MCP_AUTH_MODE_BEARER.to_string()),
            bearer_tokens: Some(vec![]),
            ..request_for_transport("http")
        };
        let err = request
            .validate_for_update(&state, Uuid::new_v4(), None, &user)
            .await
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
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
