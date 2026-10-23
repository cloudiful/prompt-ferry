//! Issue #1004 P2: the model-route test covers every eligible target.
//!
//! The test probes each enabled, currently active target of a rule once and
//! reports one outcome per target. The upstream doubles are local loopback
//! servers; no real upstream account is contacted.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Timelike;
use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db::{ConfigRepository, EndpointCreate, EndpointProvider},
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
const PATTERN_OPENCODE: &str = "gpt-model-route-test-opencode-*";
const NAME_OPENCODE: &str = "model-route-test-opencode-";
const PATTERN_SHARED_CATALOG: &str = "gpt-model-route-test-shared-catalog-*";
const NAME_SHARED_CATALOG: &str = "model-route-test-shared-catalog-";
const PATTERN_STALE_REFRESH: &str = "gpt-model-route-test-stale-refresh-*";
const NAME_STALE_REFRESH: &str = "model-route-test-stale-refresh-";
const PATTERN_ORDER: &str = "probe-order-*";
const NAME_ORDER: &str = "model-route-test-order-";
const PATTERN_ORDER_EXPLICIT: &str = "probe-order-explicit-*";

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
/// with the configured failure status. Every request is recorded with its
/// OpenCode routing session so the probe contract can be asserted.
#[derive(Clone, Debug)]
struct RecordedProbeRequest {
    path: String,
    opencode_session: Option<String>,
}

struct MockUpstream {
    base_url: String,
    requests: Arc<AtomicUsize>,
    models_requests: Arc<AtomicUsize>,
    responses_requests: Arc<AtomicUsize>,
    recorded: Arc<Mutex<Vec<RecordedProbeRequest>>>,
}

impl MockUpstream {
    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn models_request_count(&self) -> usize {
        self.models_requests.load(Ordering::SeqCst)
    }

    fn responses_request_count(&self) -> usize {
        self.responses_requests.load(Ordering::SeqCst)
    }

    fn recorded(&self) -> Vec<RecordedProbeRequest> {
        self.recorded.lock().expect("recorded requests").clone()
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn spawn_mock_upstream(models_status: u16) -> MockUpstream {
    spawn_mock_upstream_with_models(models_status, vec!["gpt-5".to_string()]).await
}

/// A loopback upstream double that answers with the given model listing, in
/// the given order.
async fn spawn_mock_upstream_with_models(models_status: u16, models: Vec<String>) -> MockUpstream {
    spawn_mock_upstream_with_session_gate(models_status, models, false).await
}

async fn spawn_mock_upstream_with_session_gate(
    models_status: u16,
    models: Vec<String>,
    require_session: bool,
) -> MockUpstream {
    let requests = Arc::new(AtomicUsize::new(0));
    let models_requests = Arc::new(AtomicUsize::new(0));
    let responses_requests = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let models_counter = requests.clone();
    let models_count = models_requests.clone();
    let models_recorded = recorded.clone();
    let responses_counter = requests.clone();
    let responses_count = responses_requests.clone();
    let responses_recorded = recorded.clone();
    let app = Router::new()
        .route(
            "/v1/models",
            get(move |headers: HeaderMap| {
                let requests = models_counter.clone();
                let models_requests = models_count.clone();
                let recorded = models_recorded.clone();
                let models = models.clone();
                async move {
                    if require_session
                        && header_value(&headers, "x-opencode-session")
                            .is_none_or(|value| value.trim().is_empty())
                    {
                        return (StatusCode::BAD_REQUEST, "MissingSessionID").into_response();
                    }
                    requests.fetch_add(1, Ordering::SeqCst);
                    models_requests.fetch_add(1, Ordering::SeqCst);
                    recorded
                        .lock()
                        .expect("recorded requests")
                        .push(RecordedProbeRequest {
                            path: "/v1/models".to_string(),
                            opencode_session: header_value(&headers, "x-opencode-session"),
                        });
                    if models_status == StatusCode::OK.as_u16() {
                        let data = models
                            .iter()
                            .map(|id| json!({ "id": id }))
                            .collect::<Vec<_>>();
                        (StatusCode::OK, axum::Json(json!({"data": data}))).into_response()
                    } else {
                        (
                            StatusCode::from_u16(models_status)
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                            axum::Json(json!({"error": "upstream failure"})),
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/v1/responses",
            post(move |headers: HeaderMap| {
                let requests = responses_counter.clone();
                let responses_requests = responses_count.clone();
                let recorded = responses_recorded.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    responses_requests.fetch_add(1, Ordering::SeqCst);
                    recorded
                        .lock()
                        .expect("recorded requests")
                        .push(RecordedProbeRequest {
                            path: "/v1/responses".to_string(),
                            opencode_session: header_value(&headers, "x-opencode-session"),
                        });
                    (StatusCode::OK, axum::Json(json!({"id": "resp_1"})))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockUpstream {
        base_url: format!("http://{address}"),
        requests,
        models_requests,
        responses_requests,
        recorded,
    }
}

/// A loopback upstream double that answers the model listing once and then
/// fails, so a refresh of a retained catalog can be observed.
async fn spawn_mock_upstream_failing_after_first(failure_status: u16) -> MockUpstream {
    let requests = Arc::new(AtomicUsize::new(0));
    let models_requests = Arc::new(AtomicUsize::new(0));
    let responses_requests = Arc::new(AtomicUsize::new(0));
    let models_answered = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let models_counter = requests.clone();
    let models_count = models_requests.clone();
    let models_state = models_answered.clone();
    let models_recorded = recorded.clone();
    let responses_counter = requests.clone();
    let responses_count = responses_requests.clone();
    let responses_recorded = recorded.clone();
    let app = Router::new()
        .route(
            "/v1/models",
            get(move |headers: HeaderMap| {
                let requests = models_counter.clone();
                let models_requests = models_count.clone();
                let answered = models_state.clone();
                let recorded = models_recorded.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    models_requests.fetch_add(1, Ordering::SeqCst);
                    recorded
                        .lock()
                        .expect("recorded requests")
                        .push(RecordedProbeRequest {
                            path: "/v1/models".to_string(),
                            opencode_session: header_value(&headers, "x-opencode-session"),
                        });
                    if answered.fetch_add(1, Ordering::SeqCst) == 0 {
                        (
                            StatusCode::OK,
                            axum::Json(json!({"data": [{"id": "gpt-5"}]})),
                        )
                    } else {
                        (
                            StatusCode::from_u16(failure_status)
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                            axum::Json(json!({"error": "upstream failure"})),
                        )
                    }
                }
            }),
        )
        .route(
            "/v1/responses",
            post(move |headers: HeaderMap| {
                let requests = responses_counter.clone();
                let responses_requests = responses_count.clone();
                let recorded = responses_recorded.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    responses_requests.fetch_add(1, Ordering::SeqCst);
                    recorded
                        .lock()
                        .expect("recorded requests")
                        .push(RecordedProbeRequest {
                            path: "/v1/responses".to_string(),
                            opencode_session: header_value(&headers, "x-opencode-session"),
                        });
                    (StatusCode::OK, axum::Json(json!({"id": "resp_1"})))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    MockUpstream {
        base_url: format!("http://{address}"),
        requests,
        models_requests,
        responses_requests,
        recorded,
    }
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

/// Issue #1052 P1: an OpenCode target with a local mock upstream.
///
/// The admin API derives the official base of a preset provider, so the row is
/// written through the config repository to keep the loopback mock as the
/// upstream. No real OpenCode account is contacted.
async fn create_opencode_endpoint(fixture: &Fixture, name: &str, base_url: &str) -> Uuid {
    let created = fixture
        .state
        .config_repository
        .create_endpoint(
            Uuid::new_v4(),
            EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: name.to_string(),
                provider: EndpointProvider::OpencodeGo,
                provider_region: None,
                service_tier: None,
                base_url: base_url.to_string(),
                native_api: NativeApi::Responses,
                native_api_source: NativeApiSource::Manual,
                api_key: "sk-opencode".to_string(),
                api_keys: Vec::new(),
                key_lb_enabled: false,
                enabled: true,
                proxy_url: None,
                active_windows: None,
            },
            false,
        )
        .await
        .expect("create the OpenCode endpoint");
    created.endpoint_id
}

#[tokio::test]
async fn an_opencode_target_probe_carries_one_session_on_both_requests() {
    // Issue #1052 P1: the OpenCode gateway rejects a request without a
    // routing session, so the probe sends one non-secret session on the
    // catalog read and on the protocol request.
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_OPENCODE, NAME_OPENCODE).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream_with_session_gate(
        StatusCode::OK.as_u16(),
        vec!["gpt-5".to_string()],
        true,
    )
    .await;
    let headerless = reqwest::Client::new()
        .get(format!("{}/v1/models", upstream.base_url))
        .send()
        .await
        .expect("loopback headerless catalog");
    assert_eq!(headerless.status(), StatusCode::BAD_REQUEST);
    assert_eq!(headerless.text().await.unwrap(), "MissingSessionID");

    let endpoint_id = create_opencode_endpoint(
        &fixture,
        &format!("{NAME_OPENCODE}target"),
        &upstream.base_url,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_OPENCODE,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(
        body["passed"], 1,
        "the OpenCode target is probed successfully"
    );

    let recorded = upstream.recorded();
    assert_eq!(
        recorded.len(),
        2,
        "the probe reads the catalog and runs the protocol request"
    );
    let sessions = recorded
        .iter()
        .map(|request| request.opencode_session.clone())
        .collect::<Vec<_>>();
    for (request, session) in recorded.iter().zip(sessions.iter()) {
        let session = session.as_deref().unwrap_or_default();
        assert!(
            !session.trim().is_empty(),
            "the probe session must not be blank: {}",
            request.path
        );
    }
    assert_eq!(
        sessions[0], sessions[1],
        "the catalog read and the protocol request share one probe session"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_generic_target_probe_carries_no_opencode_session() {
    // Issue #1052 P1: a generic provider on an unrelated host never receives a
    // synthetic OpenCode session.
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_OPENCODE, NAME_OPENCODE).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        &format!("{NAME_OPENCODE}generic"),
        &upstream.base_url,
        "sk-generic",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_OPENCODE,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["passed"], 1);

    for request in upstream.recorded() {
        assert!(
            request.opencode_session.is_none(),
            "a generic upstream must not receive an OpenCode session: {}",
            request.path
        );
    }

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn several_targets_of_one_endpoint_share_one_catalog_read() {
    // Issue #1052 P2: the probe reads the shared catalog cache, so several
    // targets of one endpoint cause one catalog read while every target keeps
    // its own generation request.
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_SHARED_CATALOG, NAME_SHARED_CATALOG).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        &format!("{NAME_SHARED_CATALOG}endpoint"),
        &upstream.base_url,
        "sk-shared",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_SHARED_CATALOG,
        json!([
            { "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" },
            { "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5-mini" },
        ]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["passed"], 2, "both targets are probed successfully");
    assert_eq!(
        upstream.models_request_count(),
        1,
        "the two targets share one catalog read"
    );
    assert_eq!(
        upstream.responses_request_count(),
        2,
        "every target keeps its own generation request"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

/// Issue #1052 P2 attempt 3: one cold catalog failure of a loopback upstream.
///
/// The fixture rows are removed before the assertion, so a failing assertion
/// never leaves a row behind.
async fn cold_catalog_failure(status: u16, pattern: &str, prefix: &str) -> serde_json::Value {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, pattern, prefix).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(status).await;
    let endpoint_id = create_endpoint(
        &app,
        &format!("{prefix}endpoint"),
        &upstream.base_url,
        "sk-attempt3-local",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        pattern,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;
    let body = test_route(&app, rule_id).await;
    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
    body
}

#[tokio::test]
async fn a_cold_catalog_failure_keeps_the_upstream_status() {
    // Issue #1052 P2 attempt 3: a cold catalog answer keeps its upstream
    // status, so an authentication or rate-limit answer stays diagnosable.
    for (status, pattern, prefix) in [
        (429_u16, "attempt3-cold-429-*", "attempt3-cold-429-"),
        (503_u16, "attempt3-cold-503-*", "attempt3-cold-503-"),
    ] {
        let body = cold_catalog_failure(status, pattern, prefix).await;
        assert_eq!(body["passed"], 0, "{body}");
        assert_eq!(body["results"][0]["status"], status, "{body}");
        assert_eq!(
            body["status"], status,
            "the aggregate keeps the status: {body}"
        );
    }
}

#[tokio::test]
async fn a_cold_catalog_transport_failure_has_no_status() {
    // Issue #1052 P2 attempt 3: a read that never reached the upstream has no
    // upstream status, so the diagnostic stays a transport failure.
    let fixture = Fixture::open().await;
    let pattern = "attempt3-cold-transport-*";
    let prefix = "attempt3-cold-transport-";
    cleanup_stale(&fixture.pool, pattern, prefix).await;
    let app = worker_admin::router(fixture.state.clone());
    let endpoint_id = create_endpoint(
        &app,
        &format!("{prefix}endpoint"),
        // Nothing listens on this loopback port, so the read never reaches
        // an upstream.
        "http://127.0.0.1:1",
        "sk-attempt3-local",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        pattern,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;
    let body = test_route(&app, rule_id).await;
    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;

    assert_eq!(body["passed"], 0, "{body}");
    assert_eq!(
        body["results"][0]["status"],
        Value::Null,
        "a transport failure has no upstream status: {body}"
    );
}

#[tokio::test]
async fn reviewer_cold_catalog_failure_preserves_upstream_status() {
    let fixture = Fixture::open().await;
    let pattern = "reviewer-1052-cold-status-*";
    let prefix = "reviewer-1052-cold-status-";
    cleanup_stale(&fixture.pool, pattern, prefix).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(StatusCode::UNAUTHORIZED.as_u16()).await;
    let endpoint_id = create_endpoint(
        &app,
        &format!("{prefix}endpoint"),
        &upstream.base_url,
        "sk-reviewer-local",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        pattern,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;
    let body = test_route(&app, rule_id).await;
    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
    assert_eq!(body["passed"], 0);
    assert_eq!(body["results"][0]["status"], 401, "{body}");
}

#[tokio::test]
async fn a_failed_probe_refresh_is_never_reported_as_a_fresh_catalog() {
    // Issue #1052 P2: the retained catalog keeps its own age, so a failed
    // refresh is reported as the real failure instead of the stale catalog.
    let mut fixture = Fixture::open().await;
    // A zero lifetime makes every stored catalog stale right away.
    fixture.state.endpoint_model_cache = EndpointModelCache::new(std::time::Duration::ZERO);
    cleanup_stale(&fixture.pool, PATTERN_STALE_REFRESH, NAME_STALE_REFRESH).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream_failing_after_first(StatusCode::BAD_GATEWAY.as_u16()).await;

    let endpoint_id = create_endpoint(
        &app,
        &format!("{NAME_STALE_REFRESH}endpoint"),
        &upstream.base_url,
        "sk-stale",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_STALE_REFRESH,
        json!([{ "endpoint_id": endpoint_id, "enabled": true, "upstream_model": "gpt-5" }]),
    )
    .await;

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["passed"], 1, "the first probe reads the catalog");

    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(
        body["passed"], 0,
        "a failed refresh is never reported as a fresh catalog"
    );
    assert_eq!(
        body["results"][0]["status"], 502,
        "the stale branch keeps the upstream status: {body}"
    );
    let message = body["results"][0]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("upstream failure"),
        "the probe reports the real refresh error: {message}"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_wildcard_probe_selects_the_first_upstream_model() {
    // Issue #1052 P2 attempt 2: a wildcard pattern selects the first model
    // the upstream listed, on a cache miss and on a cache hit.
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_ORDER, NAME_ORDER).await;
    cleanup_stale(&fixture.pool, PATTERN_ORDER_EXPLICIT, NAME_ORDER).await;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream_with_models(
        StatusCode::OK.as_u16(),
        vec!["probe-order-zzz".to_string(), "probe-order-aaa".to_string()],
    )
    .await;

    let endpoint_id = create_endpoint(
        &app,
        &format!("{NAME_ORDER}endpoint"),
        &upstream.base_url,
        "sk-order",
        json!([]),
        true,
    )
    .await;
    let rule_id = create_route(
        &app,
        PATTERN_ORDER,
        json!([{ "endpoint_id": endpoint_id, "enabled": true }]),
    )
    .await;

    // The first probe reads the catalog from the upstream.
    let body = test_route(&app, rule_id).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["passed"], 1);
    assert_eq!(
        body["results"][0]["model"], "probe-order-zzz",
        "the probe selects the first model the upstream listed"
    );

    // The second probe reads the cached catalog.
    let body = test_route(&app, rule_id).await;
    assert_eq!(body["passed"], 1);
    assert_eq!(
        body["results"][0]["model"], "probe-order-zzz",
        "the cached catalog keeps the upstream order"
    );
    assert_eq!(
        upstream.models_request_count(),
        1,
        "the second probe reads the cached catalog"
    );

    // An explicit upstream model still wins over the catalog.
    let explicit_rule_id = create_route(
        &app,
        PATTERN_ORDER_EXPLICIT,
        json!([{
            "endpoint_id": endpoint_id,
            "enabled": true,
            "upstream_model": "probe-order-aaa",
        }]),
    )
    .await;
    let body = test_route(&app, explicit_rule_id).await;
    assert_eq!(body["passed"], 1);
    assert_eq!(
        body["results"][0]["model"], "probe-order-aaa",
        "an explicit upstream model keeps precedence"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint_id]).await;
    cleanup_rows(&fixture.pool, explicit_rule_id, &[]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_enabled_active_target_is_tested_once() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool, PATTERN_MULTI, NAME_MULTI).await;
    let app = worker_admin::router(fixture.state.clone());

    let healthy_upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let failing_upstream = spawn_mock_upstream(StatusCode::BAD_GATEWAY.as_u16()).await;

    let healthy = create_endpoint(
        &app,
        "model-route-test-multi-healthy",
        &healthy_upstream.base_url,
        "sk-healthy",
        json!([]),
        true,
    )
    .await;
    let failing = create_endpoint(
        &app,
        "model-route-test-multi-failing",
        &failing_upstream.base_url,
        "sk-failing",
        json!([]),
        true,
    )
    .await;
    // A disabled endpoint is not an eligible target.
    let disabled = create_endpoint(
        &app,
        "model-route-test-multi-disabled",
        &healthy_upstream.base_url,
        "sk-disabled",
        json!([]),
        false,
    )
    .await;
    // A target outside its schedule window is not eligible either.
    let inactive = create_endpoint(
        &app,
        "model-route-test-multi-inactive",
        &healthy_upstream.base_url,
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
        healthy_upstream.request_count(),
        2,
        "the healthy target is probed once per request"
    );
    assert_eq!(
        failing_upstream.request_count(),
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
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let base = upstream.base_url.clone();

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
        upstream.request_count(),
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
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let base = upstream.base_url.clone();

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
        upstream.request_count(),
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
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let base = upstream.base_url.clone();

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
        upstream.request_count(),
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
    let upstream = spawn_mock_upstream(StatusCode::OK.as_u16()).await;
    let base = upstream.base_url.clone();

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
        upstream.request_count(),
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
