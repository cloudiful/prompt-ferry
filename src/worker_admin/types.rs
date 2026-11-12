#[path = "types/approvals.rs"]
mod approvals;
#[path = "types/auth_users.rs"]
mod auth_users;
#[path = "types/billing.rs"]
mod billing;
#[path = "types/cache_alert.rs"]
mod cache_alert;
mod config_audit;
mod config_export;
mod config_import;
#[path = "types/endpoints.rs"]
mod endpoints;
#[path = "types/mcp.rs"]
mod mcp;
#[path = "types/mcp_oauth.rs"]
mod mcp_oauth;
mod mcp_provider;
#[path = "types/model_routes.rs"]
mod model_routes;
#[path = "types/quota_key_observations.rs"]
mod quota_key_observations;
#[path = "types/quota_status.rs"]
mod quota_status;
#[path = "types/relays.rs"]
mod relays;
#[path = "types/settings.rs"]
mod settings;
#[path = "types/token_plan.rs"]
mod token_plan;
#[path = "types/usage.rs"]
mod usage;

pub use approvals::*;
pub use auth_users::*;
pub use billing::*;
pub use cache_alert::*;
pub use config_audit::*;
pub use config_export::*;
pub use config_import::*;
pub use endpoints::*;
pub use mcp::*;
pub use mcp_oauth::*;
pub use mcp_provider::*;
pub use model_routes::*;
pub use quota_key_observations::*;
pub use quota_status::*;
pub use relays::*;
pub use settings::*;
pub use token_plan::*;
pub use usage::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use uuid::Uuid;

    fn admin_user() -> SessionUser {
        SessionUser {
            user_id: 1,
            login_name: "admin".to_string(),
            display_name: "Admin".to_string(),
            is_admin: true,
        }
    }

    #[test]
    fn mcp_server_into_input_applies_defaults() {
        let input = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "stdio".to_string(),
            provider_kind: None,
            url: None,
            command: Some("mcpd".to_string()),
            args: None,
            env_json: None,
            bearer_tokens: None,
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
        .into_input(&admin_user(), None);

        assert_eq!(input.scope, "admin");
        assert_eq!(input.aggregate_naming_mode, "passthrough_preferred");
        assert_eq!(input.tool_filter_mode, "blacklist");
        assert_eq!(input.timeout_ms, 30_000);
        assert_eq!(input.bearer_tokens_json, serde_json::json!([]));
    }

    #[test]
    fn mcp_server_into_input_preserves_existing_tokens_when_missing() {
        let existing = db::McpServer {
            server_id: Uuid::new_v4(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!(["one", "two"]),
            http_headers_json: serde_json::json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: false,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
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
        };

        let input = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: None,
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
        .into_input(&admin_user(), Some(&existing));

        assert_eq!(input.bearer_tokens_json, serde_json::json!(["one", "two"]));
    }

    #[test]
    fn mcp_server_into_input_allows_explicit_clear_and_replace() {
        let cleared = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: Some(vec![]),
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
        .into_input(&admin_user(), None);

        let replaced = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: Some(vec![
                db::McpBearerToken {
                    token: "  one ".to_string(),
                    enabled: true,
                },
                db::McpBearerToken {
                    token: "".to_string(),
                    enabled: true,
                },
                db::McpBearerToken {
                    token: "two".to_string(),
                    enabled: false,
                },
            ]),
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
        .into_input(&admin_user(), None);

        assert_eq!(cleared.bearer_tokens_json, serde_json::json!([]));
        assert_eq!(
            replaced.bearer_tokens_json,
            serde_json::json!([
                { "token": "one", "enabled": true },
                { "token": "two", "enabled": false }
            ])
        );
    }

    #[test]
    fn lifecycle_manual_protocol_version_none_preserves_existing_and_empty_clears() {
        let existing = db::McpServer {
            server_id: Uuid::new_v4(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: false,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: Some("2025-06-18".to_string()),
            lifecycle_learned_mode: None,
            lifecycle_learned_protocol_version: None,
            lifecycle_learned_for_updated_at: None,
            lifecycle_learned_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // None (field omitted) keeps the existing pinned version.
        let preserved = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: None,
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: None,
        }
        .into_input(&admin_user(), Some(&existing));
        assert_eq!(
            preserved.lifecycle_manual_protocol_version.as_deref(),
            Some("2025-06-18")
        );

        // Empty string (frontend clearing the field) removes the pinned version.
        let cleared = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: None,
            http_headers_json: None,
            auth_mode: None,
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            has_proxy_url: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            enabled: None,
            timeout_ms: None,
            lifecycle_policy: None,
            lifecycle_manual_protocol_version: Some("  ".to_string()),
        }
        .into_input(&admin_user(), Some(&existing));
        assert_eq!(cleared.lifecycle_manual_protocol_version, None);
    }
}

#[cfg(test)]
mod provider_contract_tests;
