use super::*;
use axum::routing::put;
use axum::{
    extract::Request,
    middleware::{self, Next},
};
use std::{env, path::PathBuf};
use tower_http::services::{ServeDir, ServeFile};

pub async fn run_admin_server(state: AdminState, bind: &str) -> anyhow::Result<()> {
    let bind: SocketAddr = bind.parse()?;
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "worker admin listening");
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn router(state: AdminState) -> Router {
    router_with_frontend_dist(state, frontend_dist_dir())
}

fn router_with_frontend_dist(state: AdminState, frontend_dist: PathBuf) -> Router {
    let api = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route(
            "/me/client-keys",
            get(me::list_client_keys).post(me::create_client_key),
        )
        .route(
            "/me/client-keys/{key_id}",
            patch(me::update_client_key).delete(me::delete_client_key),
        )
        .route("/me/models", get(me::list_available_models))
        .route("/admin/users", get(list_users).post(create_user))
        .route("/admin/users/options", get(list_user_options))
        .route(
            "/admin/users/{user_id}",
            patch(update_user).delete(delete_user),
        )
        .route(
            "/admin/users/{user_id}/reset-password",
            post(reset_password),
        )
        .route(
            "/admin/users/{user_id}/client-keys",
            get(list_client_keys).post(create_client_key),
        )
        .route(
            "/admin/users/{user_id}/client-keys/{key_id}",
            patch(update_client_key).delete(delete_client_key),
        )
        .route(
            "/admin/endpoints",
            get(list_endpoints).post(create_endpoint),
        )
        .route(
            "/admin/endpoints/{endpoint_id}",
            patch(update_endpoint).delete(delete_endpoint),
        )
        .route("/admin/endpoints/{endpoint_id}/test", post(test_endpoint))
        .route(
            "/admin/endpoints/{endpoint_id}/token-plan-usage",
            get(token_plan_usage),
        )
        .route(
            "/admin/model-routes",
            get(list_model_routes).post(create_model_route),
        )
        .route(
            "/admin/model-routes/{rule_id}",
            patch(update_model_route).delete(delete_model_route),
        )
        .route("/admin/model-routes/test", post(test_model_route))
        .route(
            "/admin/mcp-servers",
            get(list_mcp_servers).post(create_mcp_server),
        )
        .route("/admin/relays", get(list_relays).post(create_relay))
        .route(
            "/admin/relays/{relay_id}",
            get(get_relay).patch(update_relay).delete(delete_relay),
        )
        .route("/admin/relays/{relay_id}/reconnect", post(reconnect_relay))
        .route(
            "/admin/mcp-servers/{server_id}",
            patch(update_mcp_server).delete(delete_mcp_server),
        )
        .route(
            "/admin/mcp-servers/{server_id}/catalog",
            get(get_mcp_catalog),
        )
        .route("/admin/mcp-servers/{server_id}/test", post(test_mcp_server))
        .route(
            "/admin/mcp-servers/{server_id}/credentials",
            get(list_server_credentials),
        )
        .route(
            "/admin/mcp-servers/{server_id}/credentials/{credential_id}/quota-group",
            put(bind_credential_group),
        )
        .route(
            "/admin/mcp-quota-groups",
            get(list_quota_groups).post(create_quota_group),
        )
        .route(
            "/admin/mcp-quota-groups/{group_id}",
            patch(update_quota_group).delete(delete_quota_group),
        )
        .route(
            "/admin/mcp-quota-groups/{group_id}/usage",
            get(quota_group_usage),
        )
        .route("/admin/request-records/summary", get(usage_summary))
        .route("/admin/request-records/overview", get(usage_overview))
        .route("/admin/request-records", get(usage_events))
        .route("/admin/request-records/facets", get(usage_facets))
        .route("/admin/request-records/clear", post(clear_usage_events))
        .route(
            "/admin/request-records/{record_id}",
            get(usage_event_detail),
        )
        .route(
            "/admin/request-records/{record_id}/session-route-options",
            get(usage_event_session_route_options),
        )
        .route(
            "/admin/request-records/{record_id}/reset-session-affinity",
            post(usage_event_session_affinity_reset),
        )
        .route(
            "/admin/request-records/{record_id}/request-full",
            get(usage_request_full),
        )
        .route("/admin/request-records/series", get(usage_series))
        .route("/admin/request-records/prune", post(prune_usage_events))
        .route(
            "/admin/billing/price-rules",
            get(list_billing_price_rules).post(create_billing_price_rule),
        )
        .route(
            "/admin/billing/price-rules/{price_rule_id}",
            patch(patch_billing_price_rule)
                .put(update_billing_price_rule)
                .delete(delete_billing_price_rule),
        )
        .route("/admin/billing/summary", get(billing_summary))
        .route("/admin/billing/charges", get(list_billing_charges))
        .route(
            "/admin/billing/charges/{charge_id}",
            get(billing_charge_detail),
        )
        .route("/admin/billing/reprice-unpriced", post(reprice_billing))
        .route("/admin/billing/export", get(export_billing))
        .route(
            "/admin/conversations/{conversation_id}/endpoint-override",
            get(get_conversation_endpoint_override)
                .put(set_conversation_endpoint_override)
                .delete(delete_conversation_endpoint_override),
        )
        .route(
            "/settings/endpoint",
            get(get_endpoint_setting).patch(set_endpoint_setting),
        )
        .route(
            "/settings/redaction",
            get(get_redaction_setting).patch(set_redaction_setting),
        )
        .route(
            "/settings/redaction/custom-strings",
            get(list_redaction_custom_strings),
        )
        .route("/settings/redaction/preview", post(preview_redaction))
        .route(
            "/settings/request-content-logging",
            get(get_request_content_logging).patch(set_request_content_logging),
        )
        .route(
            "/settings/usage-retention",
            get(get_usage_retention).patch(set_usage_retention),
        )
        .route(
            "/settings/stream-delta-batching",
            get(get_stream_delta_batching).patch(set_stream_delta_batching),
        )
        .route(
            "/settings/model-route-whitelist",
            get(get_model_route_whitelist).patch(set_model_route_whitelist),
        )
        .route(
            "/settings/relay-ip-whitelist",
            get(get_relay_ip_whitelist).patch(set_relay_ip_whitelist),
        )
        .route(
            "/settings/llm-review",
            get(get_llm_review_setting).patch(set_llm_review_setting),
        )
        .route("/admin/approvals", get(list_approvals))
        .route("/admin/approvals/{approval_id}", get(get_approval))
        .route(
            "/admin/approvals/{approval_id}/approve",
            post(approve_approval),
        )
        .route(
            "/admin/approvals/{approval_id}/reject",
            post(reject_approval),
        )
        .route("/bridge/status", get(bridge_status))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            reject_unsupported_sqlite_capabilities,
        ))
        .fallback(admin_api_fallback)
        .with_state(state.clone());

    let frontend_assets = ServeDir::new(frontend_dist.join("assets"));
    let frontend_index = ServeFile::new(frontend_dist.join("index.html"));

    Router::new()
        .nest("/api/v1", api)
        .nest_service("/assets", frontend_assets)
        .fallback_service(frontend_index)
        .layer(CorsLayer::permissive())
}

async fn reject_unsupported_sqlite_capabilities(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Response {
    if state.user_store.is_sqlite() {
        let path = request
            .uri()
            .path()
            .strip_prefix("/api/v1")
            .unwrap_or_else(|| request.uri().path());
        if let Some(capability) = db::Capability::for_path(normalize_admin_path(path))
            && !capability.sqlite_supported()
        {
            return state.capability_unavailable(capability);
        }
    }
    next.run(request).await
}

fn normalize_admin_path(path: &str) -> &str {
    // The current router routes on concrete path segments, so we only ever see
    // literal URI paths here. A simple pass-through keeps the middleware
    // flexible if the router ever changes.
    path
}

async fn admin_api_fallback(State(state): State<AdminState>) -> Response {
    if state.user_store.is_sqlite() {
        state.sqlite_capability_unavailable()
    } else {
        error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Admin API route not found",
        )
    }
}

#[cfg(test)]
mod admin_routing_tests {
    use super::normalize_admin_path;
    use crate::db::Capability;

    #[test]
    fn capability_lookup_matches_known_route_templates() {
        for (path, expected) in [
            ("/admin/endpoints", Some(Capability::Endpoints)),
            ("/admin/endpoints/test", Some(Capability::Endpoints)),
            ("/admin/model-routes", Some(Capability::ModelRoutes)),
            ("/admin/relays", Some(Capability::Relays)),
            ("/admin/relays/abc/reconnect", Some(Capability::Relays)),
            ("/me/client-keys", Some(Capability::ClientKeys)),
            ("/admin/users/7/client-keys", Some(Capability::ClientKeys)),
            ("/settings/endpoint", Some(Capability::EndpointSetting)),
            ("/settings/redaction", Some(Capability::Settings)),
            ("/settings/usage-retention", Some(Capability::Settings)),
            (
                "/admin/conversations/abc/endpoint-override",
                Some(Capability::ConversationEndpointOverride),
            ),
            (
                "/admin/request-records/summary",
                Some(Capability::RequestRecords),
            ),
            ("/admin/approvals", Some(Capability::Approvals)),
            ("/admin/billing/summary", Some(Capability::Billing)),
            ("/me/models", Some(Capability::AvailableModels)),
            ("/auth/me", None),
        ] {
            assert_eq!(
                Capability::for_path(normalize_admin_path(path)),
                expected,
                "path {path} mapped to {expected:?}"
            );
        }
    }
}

fn frontend_dist_dir() -> PathBuf {
    if let Ok(path) = env::var("PROMPT_FERRY_FRONTEND_DIST")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }

    ["/app/frontend/dist", "frontend/dist"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.join("index.html").is_file())
        .unwrap_or_else(|| PathBuf::from("frontend/dist"))
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
        worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use std::{fs, time::Duration};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn test_state() -> AdminState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")
            .expect("lazy pool");
        AdminState::new(AdminStateInit {
            pool: pool.clone(),
            lease_pool: pool.clone(),
            replay_cache: ReplayCache::for_tests(),
            configured_relays: vec!["ws://relay:8788/ws/worker".to_string()],
            managed_mode: false,
            relay_secret_manager: None,
            redaction_enabled: false,
            model_route_whitelist_enabled: true,
            request_content_logging: RequestContentLoggingResponse {
                mode: RequestContentLoggingMode::Off,
                raw_retention_days: 3,
            },
            usage_retention: UsageRetentionSettings::default(),
            raw_payload_store: None,
            stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: LlmReviewSettings::default(),
            mcp_catalog_cache: McpCatalogCache::new(),
            mcp_catalog_service: McpCatalogService::new(pool.clone(), McpCatalogCache::new()),
            mcp_session_store: None,
            mcp_allowed_origins: Vec::new(),
            mcp_quota_valkey: crate::mcp::McpQuotaValkey::new(),
            endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
                Duration::from_secs(60),
            ),
        })
    }

    fn temp_frontend_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prompt-ferry-frontend-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("assets")).expect("create asset dir");
        fs::write(dir.join("index.html"), "<html>relay ui</html>").expect("write index");
        fs::write(dir.join("assets/app.js"), "console.log('relay');").expect("write asset");
        dir
    }

    #[tokio::test]
    async fn router_serves_frontend_assets_and_spa_fallback() {
        let frontend_dir = temp_frontend_dir();
        let app = router_with_frontend_dist(test_state(), frontend_dir.clone());

        let asset = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        let asset_body = to_bytes(asset.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            std::str::from_utf8(&asset_body).unwrap(),
            "console.log('relay');"
        );

        let spa = app
            .oneshot(
                Request::builder()
                    .uri("/settings/relays")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(spa.status(), StatusCode::OK);
        let spa_body = to_bytes(spa.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            std::str::from_utf8(&spa_body).unwrap(),
            "<html>relay ui</html>"
        );

        let _ = fs::remove_dir_all(frontend_dir);
    }

    #[tokio::test]
    async fn sqlite_admin_api_uses_persisted_auth_and_rejects_later_capabilities() {
        let path =
            std::env::temp_dir().join(format!("prompt-ferry-admin-{}.sqlite", Uuid::new_v4()));
        let sqlite_pool = db::connect_sqlite(&path).await.expect("SQLite pool");
        db::migrate_standalone(&sqlite_pool)
            .await
            .expect("SQLite migrations");
        let user_store = db::UserStore::sqlite(sqlite_pool.clone());
        user_store
            .bootstrap_admin("admin", "admin-password")
            .await
            .expect("SQLite admin bootstrap");
        let frontend_dir = temp_frontend_dir();
        let app = router_with_frontend_dist(
            test_state().with_user_store(user_store),
            frontend_dir.clone(),
        );

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "login_name": "admin",
                            "password": "admin-password"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::NO_CONTENT);
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .expect("session cookie")
            .to_str()
            .expect("cookie value")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();

        let me_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me_response.status(), StatusCode::OK);

        let users_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/users")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(users_response.status(), StatusCode::OK);

        // Unsupported SQLite capabilities must be rejected before their
        // PostgreSQL-specific handlers touch the lazy compatibility pool.
        let unsupported = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/conversations/abc/endpoint-override")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsupported.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(unsupported.into_body(), usize::MAX).await.unwrap();
        assert!(
            std::str::from_utf8(&body)
                .expect("JSON body")
                .contains("sqlite_conversation_endpoint_override_unavailable"),
            "expected precise per-capability error, got: {}",
            std::str::from_utf8(&body).unwrap_or("<binary>")
        );

        for (path, code) in [
            (
                "/api/v1/admin/request-records/summary",
                "sqlite_request_records_unavailable",
            ),
            ("/api/v1/admin/approvals", "sqlite_approvals_unavailable"),
            (
                "/api/v1/admin/billing/summary",
                "sqlite_billing_unavailable",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::COOKIE, &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_IMPLEMENTED,
                "path {path}"
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(
                std::str::from_utf8(&body)
                    .expect("JSON body")
                    .contains(code),
                "unexpected response for {path}: {}",
                std::str::from_utf8(&body).unwrap_or("<binary>")
            );
        }

        sqlite_pool.close().await;
        let _ = fs::remove_dir_all(frontend_dir);
        let _ = fs::remove_file(path);
    }
}
