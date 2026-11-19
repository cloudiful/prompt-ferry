use super::*;
use axum::extract::DefaultBodyLimit;
use axum::http::{Extensions, Version};
use axum::{
    extract::Request,
    middleware::{self, Next},
};
#[cfg(test)]
use std::time::Duration;
use tower_http::compression::{
    CompressionLayer,
    predicate::{DefaultPredicate, NotForContentType, Predicate},
};

use crate::worker_admin::state::AdminExecutionTracker;

/// The admin router as production serves it: API first, then the frontend.
///
/// `/api/v1/*` is nested ahead of the frontend routes, so API, health, and
/// readiness routes can never be swallowed by the SPA fallback.
///
/// The frontend itself is resolved by [`crate::web_assets_server::frontend`]:
/// embedded assets by default, or a filesystem directory selected by
/// [`crate::web_assets_server::FRONTEND_DIST_ENV`] or the runtime defaults.
pub fn router(state: AdminState) -> Router {
    Router::new()
        .nest("/api/v1", api_router(state.clone()))
        .with_state(state)
        .merge(crate::web_assets_server::frontend_routes())
        .layer(CorsLayer::permissive())
        .layer(response_compression_layer())
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

/// The API surface, kept separate so the SPA fallback cannot shadow it.
///
/// Returned with [`AdminState`] still unresolved; each caller converts once
/// with `with_state` after mounting, because calling `with_state` twice on a
/// mounted router strips its routes (axum consumes the router on conversion).
fn api_router(state: AdminState) -> Router<AdminState> {
    Router::new()
        .route("/healthz", get(admin_healthz))
        .route("/ready", get(admin_ready))
        .route(
            "/auth/setup",
            get(setup_status)
                .post(setup_initialize)
                .layer(DefaultBodyLimit::max(8192)),
        )
        .route("/node", get(node_status))
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
            "/admin/endpoints/{endpoint_id}/models",
            get(list_endpoint_models),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/token-plan-usage",
            get(token_plan_usage),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/quota-inference",
            get(quota_inference),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/quota-observations",
            get(quota_key_observation_history),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/quota-observations/trend",
            get(quota_key_observation_trend),
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
        .route("/admin/nodes", get(list_nodes))
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
            "/admin/mcp-servers/{server_id}/oauth",
            get(get_mcp_oauth_status)
                .post(start_mcp_oauth)
                .delete(disconnect_mcp_oauth),
        )
        .route(
            "/admin/mcp-servers/{server_id}/oauth/callback",
            get(mcp_oauth_callback),
        )
        .route(
            "/admin/mcp-servers/{server_id}/oauth/refresh",
            post(refresh_mcp_oauth),
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
            "/settings/raw-object-store",
            get(get_raw_object_store).patch(set_raw_object_store),
        )
        .route(
            "/settings/shared-valkey",
            get(get_shared_valkey).patch(set_shared_valkey),
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
        .route("/bridge/status", get(bridge_status))
        // Issue #599 R2b: per-endpoint ChatGPT OAuth login, refresh, and clear.
        .merge(oauth::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            reject_unsupported_sqlite_capabilities,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            track_admin_execution,
        ))
        .fallback(admin_api_fallback)
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

/// Track one admin API handler and cancel it when shutdown asks.
///
/// Axum runs each connection in a separately spawned task, so the serve future
/// and its `JoinHandle` cannot prove that a business handler stopped. The
/// tracker owns the real handler future here and the shutdown path waits for
/// its count to reach zero before releasing the registry row.
async fn track_admin_execution(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Response {
    await_admin_execution(state.admin_executions.clone(), next.run(request)).await
}

async fn await_admin_execution<F>(tracker: AdminExecutionTracker, execution: F) -> Response
where
    F: std::future::Future<Output = Response>,
{
    let Some(_guard) = tracker.try_track() else {
        return admin_shutting_down_response();
    };
    // Cancellation is polled before the handler and stays first on every
    // wake-up: a handler admitted just before the close is dropped, never
    // polled after the cancellation branch is ready.
    tokio::select! {
        biased;
        _ = tracker.cancelled() => admin_shutting_down_response(),
        response = execution => response,
    }
}

fn admin_shutting_down_response() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "worker_shutting_down",
        "the node is shutting down",
    )
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
            ("/admin/nodes", Some(Capability::Workers)),
            ("/me/client-keys", Some(Capability::ClientKeys)),
            ("/admin/users/7/client-keys", Some(Capability::ClientKeys)),
            ("/settings/endpoint", Some(Capability::EndpointSetting)),
            ("/settings/redaction", Some(Capability::Settings)),
            ("/settings/usage-retention", Some(Capability::Settings)),
            (
                "/settings/raw-object-store",
                Some(Capability::RawObjectStore),
            ),
            ("/settings/shared-valkey", Some(Capability::SharedValkey)),
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
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn cancelling_shutdown_drops_the_tracked_admin_handler() {
        struct DropProbe(Arc<AtomicUsize>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicUsize::new(0));
        let probe = dropped.clone();
        let tracker = AdminExecutionTracker::default();
        let task_tracker = tracker.clone();
        let task = tokio::spawn(async move {
            await_admin_execution(task_tracker, async move {
                let _probe = DropProbe(probe);
                std::future::pending::<Response>().await
            })
            .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(tracker.active(), 1, "the handler is tracked");
        tracker.cancel();

        let response = task.await.expect("tracked handler task ran");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "cancellation drops the real handler future"
        );
        assert_eq!(tracker.active(), 0, "the tracked count returned to zero");
    }

    #[tokio::test]
    async fn a_closed_admin_tracker_refuses_admission_without_polling_the_handler() {
        let tracker = AdminExecutionTracker::default();
        let polls = Arc::new(AtomicUsize::new(0));
        tracker.cancel();

        let handler_polls = polls.clone();
        let response = await_admin_execution(tracker.clone(), async move {
            handler_polls.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK.into_response()
        })
        .await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            polls.load(Ordering::SeqCst),
            0,
            "a refused admission never polls the business handler"
        );
        assert_eq!(tracker.active(), 0);
    }

    #[tokio::test]
    async fn a_handler_admitted_just_before_the_close_is_dropped_before_it_is_polled_again() {
        let tracker = AdminExecutionTracker::default();
        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));

        let handler_started = started.clone();
        let handler_finished = finished.clone();
        let task = tokio::spawn({
            let tracker = tracker.clone();
            async move {
                await_admin_execution(tracker, async move {
                    handler_started.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    handler_finished.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK.into_response()
                })
                .await
            }
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(started.load(Ordering::SeqCst), 1, "the handler was polled");
        assert_eq!(tracker.active(), 1);

        tracker.cancel();
        let response = task.await.expect("tracked handler task ran");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            finished.load(Ordering::SeqCst),
            0,
            "the cancelled handler future was dropped, not completed"
        );
        assert_eq!(tracker.active(), 0);
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
}
