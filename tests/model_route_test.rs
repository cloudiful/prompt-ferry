//! Issue #1004 P2: the model-route test covers every eligible target.
//!
//! The test probes each enabled, currently active target of a rule once and
//! reports one outcome per target. The upstream doubles are local loopback
//! servers; no real upstream account is contacted.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use chrono::Timelike;
use prompt_ferry::{
    db::ConfigRepository,
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    worker_admin,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

const SESSION_KEY: &str = "model-route-test-session";

const PATTERN_MULTI: &str = "gpt-model-route-test-multi-*";
const PATTERN_NO_KEY: &str = "gpt-model-route-test-nokey-*";
const PATTERN_INACTIVE: &str = "gpt-model-route-test-inactive-*";
const NAME_MULTI: &str = "model-route-test-multi-";
const NAME_NO_KEY: &str = "model-route-test-nokey-";
const NAME_INACTIVE: &str = "model-route-test-inactive-";
const PATTERN_DISABLED_TARGET: &str = "gpt-model-route-test-disabled-target-*";
const PATTERN_DISABLED_ENDPOINT: &str = "gpt-model-route-test-disabled-endpoint-*";
const NAME_DISABLED_TARGET: &str = "model-route-test-disabled-target-";
const NAME_DISABLED_ENDPOINT: &str = "model-route-test-disabled-endpoint-";

/// The dedicated quota test database, reached only after the dedicated URL
/// and the operator-confirmed target agree.
async fn test_pool() -> sqlx::PgPool {
    test_pg_target::required_pool(4)
        .await
        .expect("connect to the confirmed quota test database")
}

struct Fixture {
    state: AdminState,
    pool: sqlx::PgPool,
}

impl Fixture {
    async fn open() -> Self {
        let pool = test_pool().await;
        let state = AdminState::new(AdminStateInit {
            pool: pool.clone(),
            lease_pool: pool.clone(),
            replay_cache: ReplayCache::for_tests(),
            configured_relays: Vec::new(),
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
            stream_delta_batching: prompt_ferry::db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: LlmReviewSettings::default(),
            mcp_catalog_cache: McpCatalogCache::new(),
            mcp_catalog_service: McpCatalogService::new(pool.clone(), McpCatalogCache::new()),
            mcp_session_store: None,
            mcp_allowed_origins: Vec::new(),
            endpoint_model_cache: EndpointModelCache::new(std::time::Duration::from_secs(60)),
        })
        .with_config_repository(ConfigRepository::postgres(&pool));
        state
            .replay_cache
            .write_session(
                SESSION_KEY,
                &SessionUser {
                    user_id: 1,
                    login_name: "model-route-admin".to_string(),
                    display_name: "Model Route Admin".to_string(),
                    is_admin: true,
                },
            )
            .await
            .expect("admin session");
        Self { state, pool }
    }

    async fn cleanup(self) {
        self.pool.close().await;
    }
}

/// Remove the rows a previous run of this test left behind, so the shared test
/// database keeps no leftover endpoint or rule.
async fn cleanup_stale(pool: &sqlx::PgPool, pattern: &str, name_prefix: &str) {
    sqlx::query(
        "DELETE FROM model_route_targets WHERE rule_id IN \
         (SELECT rule_id FROM model_endpoint_rules WHERE model_pattern = $1)",
    )
    .bind(pattern)
    .execute(pool)
    .await
    .expect("delete stale route targets");
    sqlx::query("DELETE FROM model_endpoint_rules WHERE model_pattern = $1")
        .bind(pattern)
        .execute(pool)
        .await
        .expect("delete stale route rules");
    sqlx::query(
        "DELETE FROM endpoint_api_keys WHERE endpoint_id IN \
         (SELECT endpoint_id FROM provider_endpoints WHERE name LIKE $1)",
    )
    .bind(format!("{name_prefix}%"))
    .execute(pool)
    .await
    .expect("delete stale endpoint keys");
    sqlx::query("DELETE FROM provider_endpoints WHERE name LIKE $1")
        .bind(format!("{name_prefix}%"))
        .execute(pool)
        .await
        .expect("delete stale endpoints");
}

/// Remove the rows the test created, so the shared test database keeps no
/// leftover endpoint or rule.
async fn cleanup_rows(pool: &sqlx::PgPool, rule_id: Uuid, endpoint_ids: &[Uuid]) {
    sqlx::query("DELETE FROM model_route_targets WHERE rule_id = $1")
        .bind(rule_id)
        .execute(pool)
        .await
        .expect("delete route targets");
    sqlx::query("DELETE FROM model_endpoint_rules WHERE rule_id = $1")
        .bind(rule_id)
        .execute(pool)
        .await
        .expect("delete route rule");
    sqlx::query("DELETE FROM endpoint_api_keys WHERE endpoint_id = ANY($1)")
        .bind(endpoint_ids)
        .execute(pool)
        .await
        .expect("delete endpoint keys");
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = ANY($1)")
        .bind(endpoint_ids)
        .execute(pool)
        .await
        .expect("delete endpoints");
}

fn request(method: &str, uri: String, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri).header(
        header::COOKIE,
        format!("prompt_ferry_session={SESSION_KEY}"),
    );
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("request")
}

async fn response_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn create_endpoint(
    app: &Router,
    name: &str,
    base_url: &str,
    api_key: &str,
    api_keys: Value,
    enabled: bool,
) -> Uuid {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/endpoints".to_string(),
            Some(json!({
                "scope": "admin",
                "owner_user_id": null,
                "name": name,
                "provider": "generic",
                "provider_region": null,
                "plan": "platform_api_key",
                "service_tier": null,
                "base_url": base_url,
                "api_key": api_key,
                "api_keys": api_keys,
                "key_lb_enabled": false,
                "protocol_mode": "manual",
                "native_api_override": "responses",
                "enabled": enabled,
                "mcp_enabled": false,
            })),
        ))
        .await
        .expect("create endpoint request");
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "endpoint create failed: {body}");
    body["endpoint_id"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("endpoint id")
}

async fn create_route(app: &Router, pattern: &str, targets: Value) -> Uuid {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/model-routes".to_string(),
            Some(json!({
                "scope": "admin",
                "owner_user_id": null,
                "model_pattern": pattern,
                "routing_strategy": "responses_session_affinity",
                "enabled": true,
                "endpoint_id": null,
                "priority": null,
                "targets": targets,
            })),
        ))
        .await
        .expect("create route request");
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "route create failed: {body}");
    body["rule_id"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("rule id")
}

async fn test_route_status(app: &Router, rule_id: Uuid) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/model-routes/test".to_string(),
            Some(json!({ "rule_id": rule_id })),
        ))
        .await
        .expect("route test request");
    let status = response.status();
    let body = response_json(response).await;
    (status, body)
}

async fn test_route(app: &Router, rule_id: Uuid) -> Value {
    let (status, body) = test_route_status(app, rule_id).await;
    assert_eq!(status, StatusCode::OK, "route test failed: {body}");
    body
}

/// A loopback upstream double. The models listing answers with one model, or
/// with the configured failure status.
async fn spawn_mock_upstream(models_status: u16) -> (String, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let models_requests = requests.clone();
    let responses_requests = requests.clone();
    let app = Router::new()
        .route(
            "/v1/models",
            get(move || {
                let requests = models_requests.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    if models_status == StatusCode::OK.as_u16() {
                        (
                            StatusCode::OK,
                            axum::Json(json!({"data": [{"id": "gpt-5"}]})),
                        )
                    } else {
                        (
                            StatusCode::from_u16(models_status)
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                            axum::Json(json!({"error": "upstream failure"})),
                        )
                    }
                }
            }),
        )
        .route(
            "/v1/responses",
            post(move || {
                let requests = responses_requests.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, axum::Json(json!({"id": "resp_1"})))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), requests)
}

/// A window that excludes the current worker-local time.
fn inactive_window() -> Value {
    let now = chrono::Local::now();
    let minutes = now.hour() as i64 * 60 + now.minute() as i64;
    let start = (minutes + 30) % (24 * 60);
    let end = (start + 1) % (24 * 60);
    json!([{
        "start": format!("{:02}:{:02}", start / 60, start % 60),
        "end": format!("{:02}:{:02}", end / 60, end % 60),
    }])
}

#[tokio::test]
async fn every_enabled_active_target_is_tested_once() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_MULTI, NAME_MULTI).await;
    let app = worker_admin::router(fixture.state.clone());

    let (healthy_base, healthy_requests) = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let (failing_base, failing_requests) =
        spawn_mock_upstream(StatusCode::BAD_GATEWAY.as_u16()).await;

    let healthy = create_endpoint(
        &app,
        "model-route-test-multi-healthy",
        &healthy_base,
        "sk-healthy",
        json!([]),
        true,
    )
    .await;
    let failing = create_endpoint(
        &app,
        "model-route-test-multi-failing",
        &failing_base,
        "sk-failing",
        json!([]),
        true,
    )
    .await;
    // A disabled endpoint is not an eligible target.
    let disabled = create_endpoint(
        &app,
        "model-route-test-multi-disabled",
        &healthy_base,
        "sk-disabled",
        json!([]),
        false,
    )
    .await;
    // A target outside its schedule window is not eligible either.
    let inactive = create_endpoint(
        &app,
        "model-route-test-multi-inactive",
        &healthy_base,
        "sk-inactive",
        json!([]),
        true,
    )
    .await;

    let rule_id = create_route(
        &app,
        PATTERN_MULTI,
        json!([
            { "endpoint_id": healthy, "enabled": true, "upstream_model": "gpt-5" },
            { "endpoint_id": failing, "enabled": true, "upstream_model": "gpt-5" },
            { "endpoint_id": disabled, "enabled": true, "upstream_model": "gpt-5" },
            {
                "endpoint_id": inactive,
                "enabled": true,
                "upstream_model": "gpt-5",
                "active_windows": inactive_window(),
            },
        ]),
    )
    .await;

    let body = test_route(&app, rule_id).await;

    assert_eq!(body["total"], 2, "only the eligible targets are tested");
    assert_eq!(body["passed"], 1, "the failing target is not a pass");
    assert_eq!(
        body["ok"], false,
        "one failing target is not a green result"
    );
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 2, "one outcome per eligible target");
    // The outcomes keep the configured target order.
    assert_eq!(results[0]["endpoint_id"], healthy.to_string());
    assert_eq!(results[1]["endpoint_id"], failing.to_string());
    assert_eq!(results[0]["ok"], true);
    assert_eq!(results[1]["ok"], false);
    assert_eq!(results[0]["model"], "gpt-5");
    assert!(
        results[1]["message"].as_str().unwrap_or_default().len() > 0,
        "a failed target reports a message"
    );
    // The preferred target is metadata, never a filter: at most one outcome
    // carries the mark, and it is a target of this rule.
    let preferred = results
        .iter()
        .filter(|result| result["preferred"] == json!(true))
        .count();
    assert!(
        preferred <= 1,
        "at most one outcome carries the preferred mark"
    );

    assert_eq!(
        healthy_requests.load(Ordering::SeqCst),
        2,
        "the healthy target is probed once per request"
    );
    assert_eq!(
        failing_requests.load(Ordering::SeqCst),
        1,
        "the failing target is probed once"
    );

    cleanup_rows(
        &fixture.pool,
        rule_id,
        &[healthy, failing, disabled, inactive],
    )
    .await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_target_without_an_enabled_key_is_not_probed_with_the_legacy_key() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_NO_KEY, NAME_NO_KEY).await;
    let app = worker_admin::router(fixture.state.clone());
    let (base, requests) = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        "model-route-test-nokey-disabled-key",
        &base,
        "sk-legacy",
        json!([{ "key_label": "primary", "api_key": "sk-legacy" }]),
        true,
    )
    .await;
    // The key row is disabled, so the endpoint has no usable credential even
    // though the legacy column still carries the secret.
    sqlx::query("UPDATE endpoint_api_keys SET enabled = FALSE WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(&fixture.pool)
        .await
        .expect("disable the endpoint key");
    let rule_id = create_route(
        &app,
        PATTERN_NO_KEY,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["passed"], 0);
    assert_eq!(body["ok"], false);
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ok"], false);
    let message = results[0]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("no API key is configured"),
        "the target reports the missing credential: {message}"
    );
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "a target without a usable credential is never contacted"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_route_without_an_active_target_is_not_a_green_result() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_INACTIVE, NAME_INACTIVE).await;
    let app = worker_admin::router(fixture.state.clone());
    let (base, requests) = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        "model-route-test-multi-inactive",
        &base,
        "sk-inactive",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_INACTIVE,
        json!([{
            "endpoint_id": endpoint_id,
            "enabled": true,
            "upstream_model": "gpt-5",
            "active_windows": inactive_window(),
        }]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 0);
    assert_eq!(body["passed"], 0);
    assert_eq!(body["ok"], false, "an untested route is never green");
    assert_eq!(body["results"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        !body["message"].as_str().unwrap_or_default().is_empty(),
        "the response explains that no target was eligible"
    );
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "an inactive target is not contacted"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_rule_with_every_target_disabled_answers_zero_eligible() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_DISABLED_TARGET, NAME_DISABLED_TARGET).await;
    let app = worker_admin::router(fixture.state.clone());
    let (base, requests) = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        "model-route-test-disabled-target-endpoint",
        &base,
        "sk-endpoint",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_DISABLED_TARGET,
        json!([{
            "endpoint_id": endpoint_id,
            "enabled": false,
            "upstream_model": "gpt-5",
        }]),
    )
    .await;

    let (status, body) = test_route_status(&app, rule_id).await;
    assert_eq!(status, StatusCode::OK, "an existing rule is not missing");
    assert_eq!(body["total"], 0);
    assert_eq!(body["passed"], 0);
    assert_eq!(
        body["ok"], false,
        "a rule without an active target is never green"
    );
    assert_eq!(body["results"].as_array().map(|rows| rows.len()), Some(0));
    assert!(
        !body["message"].as_str().unwrap_or_default().is_empty(),
        "the response explains that no target was eligible"
    );
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "a disabled target is not contacted"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_rule_with_every_endpoint_disabled_answers_zero_eligible() {
    let fixture = Fixture::open().await;
    cleanup_stale(
        &fixture.pool,
        PATTERN_DISABLED_ENDPOINT,
        NAME_DISABLED_ENDPOINT,
    )
    .await;
    let app = worker_admin::router(fixture.state.clone());
    let (base, requests) = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        "model-route-test-disabled-endpoint-endpoint",
        &base,
        "sk-endpoint",
        json!([]),
        false,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_DISABLED_ENDPOINT,
        json!([{
            "endpoint_id": endpoint_id,
            "enabled": true,
            "upstream_model": "gpt-5",
        }]),
    )
    .await;

    let (status, body) = test_route_status(&app, rule_id).await;
    assert_eq!(status, StatusCode::OK, "an existing rule is not missing");
    assert_eq!(body["total"], 0);
    assert_eq!(body["passed"], 0);
    assert_eq!(
        body["ok"], false,
        "a rule without an active target is never green"
    );
    assert_eq!(body["results"].as_array().map(|rows| rows.len()), Some(0));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "a disabled endpoint is not contacted"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unknown_rule_is_a_missing_rule() {
    let fixture = Fixture::open().await;
    let app = worker_admin::router(fixture.state.clone());

    let (status, body) = test_route_status(&app, Uuid::new_v4()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "not_found");

    fixture.cleanup().await;
}
