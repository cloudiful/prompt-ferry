#[path = "types/approvals.rs"]
mod approvals;
#[path = "types/auth_users.rs"]
mod auth_users;
#[path = "types/billing.rs"]
mod billing;
#[path = "types/endpoints.rs"]
mod endpoints;
#[path = "types/mcp.rs"]
mod mcp;
#[path = "types/model_routes.rs"]
mod model_routes;
#[path = "types/relays.rs"]
mod relays;
#[path = "types/settings.rs"]
mod settings;
#[path = "types/usage.rs"]
mod usage;

use crate::worker_admin_state::error;
use axum::{http::StatusCode, response::Response};

pub use approvals::*;
pub use auth_users::*;
pub use billing::*;
pub use endpoints::*;
pub use mcp::*;
pub use model_routes::*;
pub use relays::*;
pub use settings::*;
pub use usage::*;

fn validate_request_budget_limit(
    value: Option<i32>,
    field_name: &str,
) -> Result<(), Box<Response>> {
    if value.is_some_and(|limit| limit <= 0) {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_budget_limit",
            &format!("{field_name} must be greater than 0"),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db,
        llm_review::LlmReviewSettings,
        mcp::{McpCatalogCache, McpCatalogService},
        replay_cache::ReplayCache,
        worker_admin::AdminState,
        worker_admin_state::AdminStateInit,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
    use uuid::Uuid;

    fn test_state() -> AdminState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")
            .expect("lazy pool");
        AdminState::new(AdminStateInit {
            pool: pool.clone(),
            lease_pool: pool.clone(),
            replay_cache: ReplayCache::default(),
            configured_relays: vec!["ws://relay:8788/ws/worker".to_string()],
            managed_mode: false,
            relay_secret_manager: None,
            redaction_enabled: false,
            model_route_whitelist_enabled: true,
            request_content_logging: RequestContentLoggingResponse {
                mode: RequestContentLoggingMode::Off,
                raw_retention_days: 3,
            },
            stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: LlmReviewSettings::default(),
            mcp_catalog_cache: McpCatalogCache::new(),
            mcp_catalog_service: McpCatalogService::new(pool.clone(), McpCatalogCache::new()),
            mcp_session_store: None,
            endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
                Duration::from_secs(60),
            ),
        })
    }

    fn admin_user() -> SessionUser {
        SessionUser {
            user_id: 1,
            login_name: "admin".to_string(),
            display_name: "Admin".to_string(),
            is_admin: true,
        }
    }

    #[tokio::test]
    async fn model_route_methods_cover_create_and_update_validation_paths() {
        let state = test_state();
        let create = ModelRouteRequest {
            scope: "guest".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: None,
            session_affinity_lock_after_turns: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            endpoint_id: None,
            priority: None,
            targets: None,
        };
        assert!(create.validate_for_create(&state).await.is_err());

        let update = ModelRouteRequest {
            scope: "user".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: Some(db::ModelRouteRoutingStrategy::ResponsesSessionAffinity),
            session_affinity_lock_after_turns: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            endpoint_id: Some(Uuid::new_v4()),
            priority: None,
            targets: None,
        };
        assert!(
            update
                .validate_for_update(&state, Uuid::new_v4())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn mcp_server_methods_cover_create_and_update_validation_paths() {
        let state = test_state();
        let user = admin_user();
        let create = McpServerRequest {
            scope: Some("system".to_string()),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "stdio".to_string(),
            url: None,
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: Some(vec!["   ".to_string()]),
            http_headers_json: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            timeout_ms: None,
        };
        assert!(create.validate_for_create(&state, &user).await.is_err());

        let update = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: Some(42),
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "stdio".to_string(),
            url: None,
            command: None,
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
        };
        assert!(
            update
                .validate_for_update(&state, Uuid::new_v4(), &user)
                .await
                .is_err()
        );
    }

    #[test]
    fn mcp_server_into_input_applies_defaults() {
        let input = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "stdio".to_string(),
            url: None,
            command: Some("mcpd".to_string()),
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
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!(["one", "two"]),
            http_headers_json: serde_json::json!({}),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let input = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
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
        }
        .into_input(&admin_user(), Some(&existing));

        assert_eq!(input.bearer_tokens_json, serde_json::json!(["one", "two"]));
    }

    #[test]
    fn mcp_server_into_input_allows_explicit_clear_and_replace() {
        let cleared = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: Some(vec![]),
            http_headers_json: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            timeout_ms: None,
        }
        .into_input(&admin_user(), None);

        let replaced = McpServerRequest {
            scope: Some("admin".to_string()),
            owner_user_id: None,
            name: "catalog".to_string(),
            aggregate_naming_mode: None,
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: None,
            env_json: None,
            bearer_tokens: Some(vec![
                "  one ".to_string(),
                "".to_string(),
                "two".to_string(),
            ]),
            http_headers_json: None,
            tool_filter_mode: None,
            allowed_tools: None,
            disabled_tools: None,
            disabled_resources: None,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: None,
            timeout_ms: None,
        }
        .into_input(&admin_user(), None);

        assert_eq!(cleared.bearer_tokens_json, serde_json::json!([]));
        assert_eq!(
            replaced.bearer_tokens_json,
            serde_json::json!(["one", "two"])
        );
    }
}
