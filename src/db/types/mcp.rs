use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
pub struct McpBearerToken {
    pub token: String,
    pub enabled: bool,
}

pub const MCP_AUTH_MODE_NONE: &str = "none";
pub const MCP_AUTH_MODE_BEARER: &str = "bearer";
pub const MCP_AUTH_MODE_BASIC: &str = "basic";
pub const MCP_AUTH_MODES: [&str; 3] = [
    MCP_AUTH_MODE_NONE,
    MCP_AUTH_MODE_BEARER,
    MCP_AUTH_MODE_BASIC,
];

/// Canonical MCP provider preset ids (issue #296 Phase 1).
pub const MCP_PROVIDER_GENERIC: &str = "generic";
pub const MCP_PROVIDER_MINIMAX: &str = "minimax";
pub const MCP_PROVIDER_CONTEXT7: &str = "context7";
pub const MCP_PROVIDER_FIRECRAWL: &str = "firecrawl";

/// Known MCP provider ids. `generic` is the implicit preset for user-managed
/// http/stdio servers and also acts as the explicit "clear preset" signal.
pub const MCP_PROVIDER_KINDS: [&str; 4] = [
    MCP_PROVIDER_GENERIC,
    MCP_PROVIDER_MINIMAX,
    MCP_PROVIDER_CONTEXT7,
    MCP_PROVIDER_FIRECRAWL,
];

/// True when `value` names a known provider preset. Unknown values stay
/// readable for legacy rows but cannot be written through the admin API.
pub fn is_known_mcp_provider(value: &str) -> bool {
    MCP_PROVIDER_KINDS.contains(&value)
}

/// Display metadata for a known MCP provider preset. Transport is not part of
/// the preset: presets bind to hosted HTTP MCP endpoints while the transport
/// field keeps expressing the wire protocol (http/stdio/builtin_minimax).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
pub struct McpProviderInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Official hosted MCP endpoint URL for the preset, when one exists.
    pub default_url: Option<&'static str>,
    /// Auth style the hosted endpoint expects.
    pub auth: McpProviderAuth,
    /// Usage unit used by quota groups and usage views for this provider.
    pub unit: McpProviderUnit,
    /// Whether a public provider-balance fetch is known to exist (Phase 3).
    pub provider_balance_supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpProviderAuth {
    None,
    Bearer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpProviderUnit {
    Requests,
    Credits,
}

impl McpProviderUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Credits => "credits",
        }
    }
}

pub const MCP_PROVIDER_REGISTRY: [McpProviderInfo; 4] = [
    McpProviderInfo {
        id: MCP_PROVIDER_GENERIC,
        display_name: "Generic",
        default_url: None,
        auth: McpProviderAuth::None,
        unit: McpProviderUnit::Requests,
        provider_balance_supported: false,
    },
    McpProviderInfo {
        id: MCP_PROVIDER_MINIMAX,
        display_name: "MiniMax",
        default_url: None,
        auth: McpProviderAuth::None,
        unit: McpProviderUnit::Requests,
        provider_balance_supported: false,
    },
    McpProviderInfo {
        id: MCP_PROVIDER_CONTEXT7,
        display_name: "Context7",
        default_url: Some("https://mcp.context7.com/mcp"),
        auth: McpProviderAuth::Bearer,
        unit: McpProviderUnit::Requests,
        // Context7 exposes no stable public remaining-balance endpoint; only
        // local request accounting is shown, never a fabricated balance.
        provider_balance_supported: false,
    },
    McpProviderInfo {
        id: MCP_PROVIDER_FIRECRAWL,
        display_name: "Firecrawl",
        default_url: Some("https://mcp.firecrawl.dev/v2/mcp"),
        auth: McpProviderAuth::Bearer,
        unit: McpProviderUnit::Credits,
        provider_balance_supported: true,
    },
];

/// Registry metadata for a provider id, `None` for unknown/legacy values.
pub fn mcp_provider_info(provider_kind: Option<&str>) -> Option<&'static McpProviderInfo> {
    let id = provider_kind?.trim();
    MCP_PROVIDER_REGISTRY.iter().find(|info| info.id == id)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct McpServer {
    pub server_id: uuid::Uuid,
    pub source_endpoint_id: Option<uuid::Uuid>,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    #[schema(value_type = String, example = "passthrough_preferred")]
    pub aggregate_naming_mode: String,
    pub transport: String,
    /// Provider preset id; `None`/`generic` mean the untyped legacy behavior.
    pub provider_kind: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: serde_json::Value,
    pub env_json: serde_json::Value,
    #[schema(value_type = Vec<McpBearerToken>)]
    #[serde(rename = "bearer_tokens")]
    pub bearer_tokens_json: serde_json::Value,
    pub http_headers_json: serde_json::Value,
    pub auth_mode: String,
    pub basic_username: Option<String>,
    pub basic_password: Option<String>,
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

    pub fn effective_auth_mode(&self) -> &str {
        let mode = self.auth_mode.trim();
        if mode.is_empty() {
            // Legacy rows written before the explicit auth_mode migration keep
            // `none`/empty even when bearer tokens exist. Keep them effective
            // as bearer without requiring a manual edit.
            if !self.bearer_tokens().is_empty() {
                return MCP_AUTH_MODE_BEARER;
            }
            return MCP_AUTH_MODE_NONE;
        }
        match mode {
            MCP_AUTH_MODE_BEARER | MCP_AUTH_MODE_BASIC | MCP_AUTH_MODE_NONE => mode,
            _ => {
                if !self.bearer_tokens().is_empty() {
                    MCP_AUTH_MODE_BEARER
                } else {
                    MCP_AUTH_MODE_NONE
                }
            }
        }
    }

    pub fn has_basic_password(&self) -> bool {
        self.basic_password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    /// Canonical provider id of the row. Legacy rows (NULL/empty) and unknown
    /// stored values both resolve to `generic` so runtime behavior is
    /// unchanged until Phase 2 introduces preset execution.
    pub fn effective_provider_kind(&self) -> &str {
        effective_provider_kind(self.provider_kind.as_deref())
    }
}

/// Canonical provider id for a stored value; unknown/legacy -> `generic`.
pub fn effective_provider_kind(value: Option<&str>) -> &'static str {
    let Some(id) = value.map(str::trim) else {
        return MCP_PROVIDER_GENERIC;
    };
    if id == MCP_PROVIDER_MINIMAX {
        return MCP_PROVIDER_MINIMAX;
    }
    if id == MCP_PROVIDER_CONTEXT7 {
        return MCP_PROVIDER_CONTEXT7;
    }
    if id == MCP_PROVIDER_FIRECRAWL {
        return MCP_PROVIDER_FIRECRAWL;
    }
    MCP_PROVIDER_GENERIC
}

pub fn is_valid_auth_mode(value: &str) -> bool {
    matches!(value, "none" | "bearer" | "basic")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_with_tokens(tokens: Value) -> McpServer {
        McpServer {
            server_id: uuid::Uuid::nil(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "test".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: Value::Array(Vec::new()),
            env_json: Value::Object(Default::default()),
            bearer_tokens_json: tokens,
            http_headers_json: Value::Object(Default::default()),
            auth_mode: MCP_AUTH_MODE_NONE.to_string(),
            basic_username: None,
            basic_password: None,
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
    fn mcp_env_reference_accepts_only_environment_variable_names() {
        assert_eq!(
            mcp_env_reference_name("{env:MINIMAX_API_KEY}"),
            Some("MINIMAX_API_KEY")
        );
        assert_eq!(mcp_env_reference_name("{env:MINIMAX-API-KEY}"), None);
        assert_eq!(mcp_env_reference_name("MINIMAX_API_KEY"), None);
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
    pub source_endpoint_id: Option<uuid::Uuid>,
    pub name: String,
    pub aggregate_naming_mode: String,
    pub transport: String,
    pub provider_kind: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: serde_json::Value,
    pub env_json: serde_json::Value,
    pub bearer_tokens_json: serde_json::Value,
    pub http_headers_json: serde_json::Value,
    pub auth_mode: String,
    pub basic_username: Option<String>,
    pub basic_password: Option<String>,
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

/// Returns the worker environment variable name for the strict reference form
/// used by stdio MCP configuration, such as `{env:MINIMAX_API_KEY}`.
pub fn mcp_env_reference_name(value: &str) -> Option<&str> {
    let name = value.strip_prefix("{env:")?.strip_suffix('}')?;
    if name.is_empty()
        || !name.bytes().enumerate().all(|(index, byte)| match index {
            0 => byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_lowercase(),
            _ => {
                byte == b'_'
                    || byte.is_ascii_uppercase()
                    || byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
            }
        })
    {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn registry_contains_required_presets() {
        for id in ["generic", "minimax", "context7", "firecrawl"] {
            assert!(
                is_known_mcp_provider(id),
                "{id} must be a known provider preset"
            );
        }
        assert!(!is_known_mcp_provider("openai"));
        assert!(!is_known_mcp_provider(""));
    }

    #[test]
    fn preset_urls_match_official_endpoints() {
        let context7 = mcp_provider_info(Some("context7")).expect("context7 preset");
        assert_eq!(context7.default_url, Some("https://mcp.context7.com/mcp"));
        assert_eq!(context7.unit, McpProviderUnit::Requests);
        assert_eq!(context7.auth, McpProviderAuth::Bearer);
        assert!(!context7.provider_balance_supported);

        let firecrawl = mcp_provider_info(Some("firecrawl")).expect("firecrawl preset");
        assert_eq!(
            firecrawl.default_url,
            Some("https://mcp.firecrawl.dev/v2/mcp")
        );
        assert_eq!(firecrawl.unit, McpProviderUnit::Credits);
        assert!(firecrawl.provider_balance_supported);

        assert!(
            mcp_provider_info(Some("generic"))
                .expect("generic preset")
                .default_url
                .is_none()
        );
        assert!(mcp_provider_info(None).is_none());
        assert!(mcp_provider_info(Some("legacy-unknown")).is_none());
    }

    #[test]
    fn effective_provider_kind_falls_back_to_generic() {
        assert_eq!(effective_provider_kind(None), "generic");
        assert_eq!(effective_provider_kind(Some("  ")), "generic");
        assert_eq!(effective_provider_kind(Some("generic")), "generic");
        assert_eq!(effective_provider_kind(Some("legacy-unknown")), "generic");
        assert_eq!(effective_provider_kind(Some("context7")), "context7");
        assert_eq!(effective_provider_kind(Some(" firecrawl ")), "firecrawl");
        assert_eq!(effective_provider_kind(Some("minimax")), "minimax");
    }
}
