use super::*;
use anyhow::Context;
use axum::extract::DefaultBodyLimit;
use axum::http::{Extensions, Version};
use axum::{
    extract::Request,
    middleware::{self, Next},
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;
use tower_http::compression::{
    CompressionLayer,
    predicate::{DefaultPredicate, NotForContentType, Predicate},
};
use tower_http::services::{ServeDir, ServeFile};

use crate::config;

/// Hard ceiling on how long the admin HTTP server waits for in-flight
/// requests to drain before forcing shutdown. Sized to fit inside the
/// worker container's `stop_grace_period` (30s) alongside the rest of
/// the worker shutdown bookkeeping.
pub const ADMIN_SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

/// Run the admin HTTP server for the full lifetime of the worker process.
/// The `ADMIN_SHUTDOWN_BUDGET` only caps the drain phase *after* shutdown
/// has been signalled — a healthy worker is never cut off by the timeout.
/// When `shutdown_rx` is `None` the server falls back to listening for OS
/// signals directly (handy for `cargo run`).
///
/// `listener` is the socket to serve on. The integrated entrypoint passes one
/// it already reserved, so a fixed admin port cannot be claimed by another
/// process while the worker is still running database migrations; the
/// standalone worker passes `None` and binds `bind_address` here.
///
/// The address reported after binding is the one really served: a configured
/// port of `0` resolves here, so the startup path learns the address to open
/// instead of echoing back what it intended to bind.
pub async fn run_admin_server(
    state: AdminState,
    bind_address: &str,
    listener: Option<tokio::net::TcpListener>,
    shutdown_rx: Option<watch::Receiver<bool>>,
) -> anyhow::Result<()> {
    let listener = match listener {
        Some(listener) => listener,
        None => bind_admin_listener(bind_address).await?,
    };
    let bound_addr = listener.local_addr()?;
    config::integrated_startup::publish_bound(bound_addr);
    tracing::info!(%bound_addr, configured_bind = %bind_address, "worker admin listening");
    let app = router(state);
    // Both the graceful-shutdown future and the drain budget need to watch
    // the same fired signal. A single watch channel keeps them in sync.
    let (fired_tx, fired_rx) = watch::channel(false);
    let mut shutdown_watcher = shutdown_rx;
    tokio::spawn(async move {
        match shutdown_watcher.as_mut() {
            Some(rx) => {
                if !*rx.borrow() {
                    let _ = rx.changed().await;
                }
            }
            None => admin_shutdown_signal().await,
        }
        let _ = fired_tx.send(true);
    });
    let serve_shutdown = fired_rx.clone();
    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        wait_for_fired(serve_shutdown).await;
    });
    let drain_rx = fired_rx.clone();
    tokio::select! {
        result = serve => result.map_err(anyhow::Error::from),
        _ = drain_deadline(drain_rx, ADMIN_SHUTDOWN_BUDGET) => {
            tracing::warn!(
                budget_seconds = ADMIN_SHUTDOWN_BUDGET.as_secs(),
                "admin server exceeded graceful shutdown budget; forcing exit"
            );
            Ok(())
        }
    }
}

async fn wait_for_fired(mut rx: watch::Receiver<bool>) {
    if *rx.borrow() {
        return;
    }
    let _ = rx.changed().await;
}

/// Bind the admin socket from a configured address.
///
/// Kept separate from the serve path so a caller that must treat a bind
/// failure as a startup failure can do so before spawning the server.
async fn bind_admin_listener(bind_address: &str) -> anyhow::Result<tokio::net::TcpListener> {
    let bind_addr: SocketAddr = bind_address
        .parse()
        .with_context(|| format!("invalid worker admin bind address `{bind_address}`"))?;
    tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| {
            format!(
                "failed to bind the worker admin listener to `{bind_address}`; set port 0 to let \
                 prompt-ferry pick a free port"
            )
        })
}

/// Resolves `budget` after shutdown is signalled, and never before.
async fn drain_deadline(rx: watch::Receiver<bool>, budget: Duration) {
    wait_for_fired(rx).await;
    tokio::time::sleep(budget).await;
}

async fn admin_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The admin router as production serves it: API first, then the frontend.
///
/// `/api/v1/*` is nested ahead of the frontend routes, so API, health, and
/// readiness routes can never be swallowed by the SPA fallback.
///
/// The frontend itself is resolved by [`crate::web_assets_server::frontend`]:
/// the assets embedded in the binary by default, or the deliberate filesystem
/// override when [`crate::web_assets::FRONTEND_DIST_ENV`] points at a built
/// `dist` directory (frontend development without a Rust rebuild).
pub fn router(state: AdminState) -> Router {
    match crate::web_assets_server::frontend() {
        crate::web_assets_server::Frontend::Embedded => Router::new()
            .nest("/api/v1", api_router(state.clone()))
            .with_state(state)
            .route_service(
                "/assets/{*rest}",
                get(|Path(asset_path): Path<String>| async move {
                    crate::web_assets_server::serve_embedded_file(&format!("assets/{asset_path}"))
                        .await
                }),
            )
            .route_service(
                "/favicon.svg",
                get(|| async {
                    crate::web_assets_server::serve_embedded_file("favicon.svg").await
                }),
            )
            .fallback(get(crate::web_assets_server::serve_embedded_index))
            .layer(CorsLayer::permissive())
            .layer(response_compression_layer()),
        crate::web_assets_server::Frontend::Filesystem(dist) => {
            router_with_frontend_dist(state, dist)
        }
    }
}

fn response_compression_layer() -> CompressionLayer<impl Predicate + Send + 'static> {
    let predicate = DefaultPredicate::new()
        .and(NotForContentType::SSE)
        .and(skip_websocket_upgrade);
    CompressionLayer::new().compress_when(predicate)
}

fn skip_websocket_upgrade(
    status: StatusCode,
    _version: Version,
    headers: &HeaderMap,
    _extensions: &Extensions,
) -> bool {
    if status == StatusCode::SWITCHING_PROTOCOLS {
        return false;
    }
    if headers.contains_key(header::UPGRADE) {
        return false;
    }
    if headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
    {
        return false;
    }
    true
}

/// The filesystem-serving router, kept for the deliberate frontend-override
/// development seam and its adjacent tests. Production serves from the
/// embedded assets through [`router`].
/// The API surface, kept separate so the SPA fallback cannot shadow it.
///
/// Returned with [`AdminState`] still unresolved; each caller converts once
/// with `with_state` after mounting, because calling `with_state` twice on a
/// mounted router strips its routes (axum consumes the router on conversion).
fn api_router(state: AdminState) -> Router<AdminState> {
    Router::new()
        .route("/healthz", get(admin_healthz))
        .route("/ready", get(admin_ready))
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
            "/admin/endpoints/{endpoint_id}/organization-usage",
            get(organization_usage),
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
        .route("/admin/mcp-providers", get(list_mcp_providers))
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
            "/settings/cache-alert",
            get(get_cache_alert_setting).put(set_cache_alert_setting),
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
        .route(
            "/settings/raw-object-store",
            get(get_raw_object_store).patch(set_raw_object_store),
        )
        .route("/admin/config-export", post(export_config))
        .route(
            "/admin/config-export/metadata",
            post(config_export_metadata),
        )
        .route("/admin/config-audit", get(list_config_audit))
        // The configuration archive travels base64-encoded in a JSON body, so
        // these two routes carry their own body limit instead of the default.
        .merge(config_import_routes())
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
        // Issue #599 R2b: per-endpoint ChatGPT OAuth login, refresh, and clear.
        .merge(oauth::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            reject_unsupported_sqlite_capabilities,
        ))
        .fallback(admin_api_fallback)
}

/// The filesystem-serving router, kept for the deliberate frontend-override
/// development seam and its adjacent tests. Production serves from the
/// embedded assets through [`router`].
fn router_with_frontend_dist(state: AdminState, frontend_dist: PathBuf) -> Router {
    let frontend_assets = ServeDir::new(frontend_dist.join("assets"));
    let frontend_index = ServeFile::new(frontend_dist.join("index.html"));
    let frontend_favicon = ServeFile::new(frontend_dist.join("favicon.svg"));

    Router::new()
        .nest("/api/v1", api_router(state.clone()))
        .with_state(state)
        .nest_service("/assets", frontend_assets)
        .route_service("/favicon.svg", frontend_favicon)
        .fallback_service(frontend_index)
        .layer(CorsLayer::permissive())
        .layer(response_compression_layer())
}

/// Routes for the administrator configuration import.
///
/// The archive travels base64-encoded inside a JSON body, which is larger than
/// the default body limit. The sub-router raises the limit to exactly the
/// encodable archive size plus JSON envelope headroom, so oversized requests
/// are rejected before the handler allocates anything.
fn config_import_routes() -> Router<AdminState> {
    let limit = MAX_ARCHIVE_BASE64_LEN + 512 * 1024;
    Router::new()
        .route("/admin/config-import/preview", post(preview_config_import))
        .route("/admin/config-import", post(import_config))
        .layer(DefaultBodyLimit::max(limit))
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

/// Cheap liveness probe — returns 200 as soon as the admin HTTP server
/// is accepting connections. Does not check downstream dependencies so
/// Kubernetes / compose can keep the pod in the load balancer while a
/// cold DB warms up.
async fn admin_healthz() -> &'static str {
    "ok"
}

/// Readiness probe — verifies the worker can talk to its database. A
/// successful round-trip means migrations ran and the admin state was
/// built, which is exactly what the docker-compose healthcheck waits on
/// before letting the worker accept relay traffic.
async fn admin_ready(State(state): State<AdminState>) -> Response {
    let probe = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await;
    match probe {
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            &format!("database probe failed: {err}"),
        ),
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
            (
                "/admin/endpoints/abc/organization-usage",
                Some(Capability::Endpoints),
            ),
            ("/admin/model-routes", Some(Capability::ModelRoutes)),
            ("/admin/relays", Some(Capability::Relays)),
            ("/admin/relays/abc/reconnect", Some(Capability::Relays)),
            ("/me/client-keys", Some(Capability::ClientKeys)),
            ("/admin/users/7/client-keys", Some(Capability::ClientKeys)),
            ("/settings/endpoint", Some(Capability::EndpointSetting)),
            ("/settings/redaction", Some(Capability::Settings)),
            ("/settings/usage-retention", Some(Capability::Settings)),
            (
                "/settings/raw-object-store",
                Some(Capability::RawObjectStore),
            ),
            (
                "/admin/conversations/abc/endpoint-override",
                Some(Capability::ConversationEndpointOverride),
            ),
            (
                "/admin/request-records/summary",
                Some(Capability::RequestRecords),
            ),
            // The read-only provider registry is served from memory and stays
            // available on SQLite; it must not map to an unsupported capability.
            ("/admin/mcp-providers", None),
            ("/admin/approvals", Some(Capability::Approvals)),
            ("/admin/billing/summary", Some(Capability::Billing)),
            ("/me/models", Some(Capability::AvailableModels)),
            ("/admin/config-export", Some(Capability::ConfigExport)),
            (
                "/admin/config-export/metadata",
                Some(Capability::ConfigExport),
            ),
            ("/admin/config-import", Some(Capability::ConfigImport)),
            (
                "/admin/config-import/preview",
                Some(Capability::ConfigImport),
            ),
            ("/admin/config-audit", Some(Capability::ConfigAudit)),
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
    use std::{fs, time::Duration};
    use tower::ServiceExt;
    use uuid::Uuid;

    #[allow(dead_code)]
    mod test_db_url {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/test_db_url.rs"
        ));
    }

    fn test_state() -> AdminState {
        let pool = test_db_url::lazy_test_pool();
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

    #[tokio::test]
    async fn router_healthz_endpoint_returns_ok_without_database() {
        let frontend_dir = temp_frontend_dir();
        let app = router_with_frontend_dist(test_state(), frontend_dir.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "ok");
        let _ = fs::remove_dir_all(frontend_dir);
    }

    #[tokio::test]
    async fn response_compression_layer_compresses_json_but_not_sse_or_upgrade() {
        use axum::response::{IntoResponse, Response};
        use axum::routing::get;

        async fn json_handler() -> Response {
            axum::Json(serde_json::json!({
                "data": "x".repeat(512),
                "message": "compressible admin json payload",
            }))
            .into_response()
        }

        async fn sse_handler() -> Response {
            let body = format!("data: {}\n\n", "x".repeat(512));
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from(body),
            )
                .into_response()
        }

        async fn upgrade_handler() -> Response {
            (
                StatusCode::SWITCHING_PROTOCOLS,
                [
                    (header::UPGRADE, "websocket"),
                    (header::CONNECTION, "Upgrade"),
                ],
                Body::from("x".repeat(512)),
            )
                .into_response()
        }

        let app = axum::Router::new()
            .route("/json", get(json_handler))
            .route("/sse", get(sse_handler))
            .route("/ws", get(upgrade_handler))
            .layer(super::response_compression_layer());

        let json_gzip = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/json")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json_gzip
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip"),
            "JSON should gain Content-Encoding: gzip",
        );

        let json_br = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/json")
                    .header(header::ACCEPT_ENCODING, "br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json_br
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("br"),
            "JSON should gain Content-Encoding: br",
        );

        let json_plain = app
            .clone()
            .oneshot(Request::builder().uri("/json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(
            json_plain.headers().get(header::CONTENT_ENCODING).is_none(),
            "JSON should stay uncompressed without Accept-Encoding",
        );

        let sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/sse")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            sse.headers().get(header::CONTENT_ENCODING).is_none(),
            "text/event-stream must stay uncompressed",
        );

        let upgrade = app
            .oneshot(
                Request::builder()
                    .uri("/ws")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(upgrade.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert!(
            upgrade.headers().get(header::CONTENT_ENCODING).is_none(),
            "websocket upgrade must stay uncompressed",
        );
    }

    #[tokio::test]
    async fn admin_router_compresses_json_when_client_advertises_gzip() {
        let frontend_dir = temp_frontend_dir();
        let app = router_with_frontend_dist(test_state(), frontend_dir.clone());

        let compressed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(compressed.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            compressed
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip"),
            "admin JSON should gain Content-Encoding: gzip through the router layer",
        );

        let plain = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(plain.status(), StatusCode::UNAUTHORIZED);
        assert!(
            plain.headers().get(header::CONTENT_ENCODING).is_none(),
            "admin JSON should stay uncompressed without Accept-Encoding",
        );

        let _ = fs::remove_dir_all(frontend_dir);
    }

    /// Build the embedded frontend router even when the checkout carries a
    /// `frontend/dist` that would otherwise select the filesystem override.
    fn embedded_router(state: AdminState) -> Router {
        Router::new()
            .nest("/api/v1", api_router(state.clone()))
            .with_state(state)
            .route_service(
                "/assets/{*rest}",
                get(|Path(asset_path): Path<String>| async move {
                    crate::web_assets_server::serve_embedded_file(&format!("assets/{asset_path}"))
                        .await
                }),
            )
            .route_service(
                "/favicon.svg",
                get(|| async {
                    crate::web_assets_server::serve_embedded_file("favicon.svg").await
                }),
            )
            .fallback(get(crate::web_assets_server::serve_embedded_index))
            .layer(CorsLayer::permissive())
            .layer(response_compression_layer())
    }

    /// The embedded router must serve the SPA entry from the binary — the
    /// built page when the compile captured one, the fallback page otherwise.
    #[tokio::test]
    async fn embedded_router_serves_the_spa_entry_and_hashed_assets() {
        let app = embedded_router(test_state());

        let index = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(index.status(), StatusCode::OK);
        let served_type = index
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .expect("the entry has a content type");
        assert!(
            served_type.starts_with("text/html"),
            "the entry must be HTML, got {served_type}"
        );
        let index_body = to_bytes(index.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&index_body).expect("the entry is UTF-8");
        assert!(
            html.contains("<!doctype html") || html.contains("<html"),
            "the embedded entry must be HTML, got: {html}"
        );

        // A no-dist build serves the fallback page, which names the remedy.
        if !crate::web_assets::has_embedded_index() {
            assert!(
                html.contains("not built into this binary"),
                "the no-dist fallback must say so, got: {html}"
            );
        }

        // A hashed asset the built dist actually carries is served from the
        // binary with the immutable cache policy; a no-dist build has none and
        // every asset path 404s instead of falling back to the entry.
        match crate::web_assets::hashed_js_asset_path() {
            Some(asset_path) => {
                let asset = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("/{asset_path}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(asset.status(), StatusCode::OK, "asset {asset_path}");
                assert_eq!(
                    asset
                        .headers()
                        .get(header::CACHE_CONTROL)
                        .and_then(|value| value.to_str().ok()),
                    Some(crate::web_assets_server::IMMUTABLE_CACHE_CONTROL),
                    "a hashed asset must carry the immutable cache policy"
                );
                assert_eq!(
                    asset
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok()),
                    Some("text/javascript; charset=utf-8"),
                    "the asset content type names JavaScript with the charset"
                );
            }
            None => {
                let no_assets = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri("/assets/anything.js")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    no_assets.status(),
                    StatusCode::NOT_FOUND,
                    "a no-dist build must not serve assets"
                );
            }
        }

        // An unknown asset is a 404, not an SPA fallback.
        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/assets/does-not-exist.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn embedded_router_serves_the_favicon_and_keeps_api_routes_ahead_of_the_spa() {
        let app = embedded_router(test_state());

        let favicon = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/favicon.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if crate::web_assets::has_embedded_index() {
            assert_eq!(favicon.status(), StatusCode::OK);
            assert_eq!(
                favicon
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok()),
                Some("image/svg+xml")
            );
        } else {
            assert_eq!(
                favicon.status(),
                StatusCode::NOT_FOUND,
                "a no-dist build embeds no favicon"
            );
        }

        // Health stays reachable ahead of the SPA fallback.
        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        // An unknown API route must return the API fallback, never the SPA.
        let unknown_api = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/definitely-not-a-route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown_api.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(unknown_api.into_body(), usize::MAX).await.unwrap();
        let body = std::str::from_utf8(&body).unwrap_or_default();
        assert!(
            body.contains("not_found"),
            "an unknown API route must answer as JSON, got: {body}"
        );

        // A deep SPA history route falls back to the entry point.
        let spa_route = app
            .oneshot(
                Request::builder()
                    .uri("/settings/relays")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(spa_route.status(), StatusCode::OK);
        let spa_type = spa_route
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .expect("the SPA route has a content type");
        assert!(
            spa_type.starts_with("text/html"),
            "a history route must resolve to the SPA entry, got {spa_type}"
        );
    }
}
