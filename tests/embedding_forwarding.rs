//! Issue #1032 P2: embeddings forwarding, discovery and the admin probe.
//!
//! The upstream double is a local loopback server; no real upstream account,
//! credential, or provider is contacted. The route probe and the model
//! catalog both run through the real admin router against the confirmed
//! dedicated test database.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use prompt_ferry::{
    config::{BridgeEncryptionMode, NativeApi, RelayConfig, WorkerConfig},
    db::ConfigRepository,
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    relay::{self, RelayHandle},
    replay_cache::ReplayCache,
    worker, worker_admin,
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

const SESSION_KEY: &str = "embedding-forwarding-session";
const PATTERN: &str = "qwen/embedding-forwarding-*";
const NAME_PREFIX: &str = "embedding-forwarding-";
const EMBEDDING_MODEL: &str = "qwen/qwen3-embedding-8b";

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
                    login_name: "embedding-forwarding-admin".to_string(),
                    display_name: "Embedding Forwarding Admin".to_string(),
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

async fn cleanup_stale(pool: &sqlx::PgPool) {
    sqlx::query(
        "DELETE FROM model_route_targets WHERE rule_id IN \
         (SELECT rule_id FROM model_endpoint_rules WHERE model_pattern = $1)",
    )
    .bind(PATTERN)
    .execute(pool)
    .await
    .expect("delete stale route targets");
    sqlx::query("DELETE FROM model_endpoint_rules WHERE model_pattern = $1")
        .bind(PATTERN)
        .execute(pool)
        .await
        .expect("delete stale route rules");
    sqlx::query(
        "DELETE FROM endpoint_api_keys WHERE endpoint_id IN \
         (SELECT endpoint_id FROM provider_endpoints WHERE name LIKE $1)",
    )
    .bind(format!("{NAME_PREFIX}%"))
    .execute(pool)
    .await
    .expect("delete stale endpoint keys");
    sqlx::query("DELETE FROM provider_endpoints WHERE name LIKE $1")
        .bind(format!("{NAME_PREFIX}%"))
        .execute(pool)
        .await
        .expect("delete stale endpoints");
}

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

async fn create_embeddings_endpoint(app: &Router, name: &str, base_url: &str) -> Uuid {
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
                "api_key": "sk-embedding",
                "api_keys": [],
                "key_lb_enabled": false,
                "protocol_mode": "manual",
                "native_api_override": "embeddings",
                "enabled": true,
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

async fn create_route(app: &Router, targets: Value) -> Uuid {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/model-routes".to_string(),
            Some(json!({
                "scope": "admin",
                "owner_user_id": null,
                "model_pattern": PATTERN,
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

#[tokio::test]
async fn reviewer_omitted_dimensions_preserve_each_same_endpoint_target() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool).await;
    let app = worker_admin::router(fixture.state.clone());
    let endpoint = create_embeddings_endpoint(
        &app,
        &format!("{NAME_PREFIX}reviewer-carry"),
        "http://127.0.0.1:1",
    )
    .await;
    let rule_id = create_route(
        &app,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a", "embedding_dimensions_override": 512},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b", "embedding_dimensions_override": 1024}
        ]),
    )
    .await;
    let response = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/model-routes/{rule_id}"),
            Some(json!({
                "scope": "admin", "owner_user_id": null, "model_pattern": PATTERN,
                "enabled": true, "routing_strategy": "responses_session_affinity",
                "targets": [
                    {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a"},
                    {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b"}
                ]
            })),
        ))
        .await
        .expect("update route");
    let status = response.status();
    let body = response_json(response).await;
    let rule = prompt_ferry::db::get_model_endpoint_rule(&fixture.pool, rule_id)
        .await
        .expect("read route")
        .expect("route exists");
    let dimensions: Vec<_> = rule
        .targets
        .iter()
        .map(|target| target.embedding_dimensions_override)
        .collect();
    cleanup_rows(&fixture.pool, rule_id, &[endpoint]).await;
    fixture.cleanup().await;
    assert_eq!(status, StatusCode::OK, "update failed: {body}");
    assert_eq!(dimensions, vec![Some(512), Some(1024)]);
}

async fn patch_route(app: &Router, rule_id: Uuid, targets: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/model-routes/{rule_id}"),
            Some(json!({
                "scope": "admin",
                "owner_user_id": null,
                "model_pattern": PATTERN,
                "enabled": true,
                "routing_strategy": "responses_session_affinity",
                "targets": targets,
            })),
        ))
        .await
        .expect("update route");
    let status = response.status();
    (status, response_json(response).await)
}

async fn stored_dimensions(
    pool: &sqlx::PgPool,
    rule_id: Uuid,
) -> Vec<(Option<String>, Option<i32>)> {
    sqlx::query_as(
        "SELECT upstream_model, embedding_dimensions_override FROM model_route_targets \
         WHERE rule_id = $1 ORDER BY position",
    )
    .bind(rule_id)
    .fetch_all(pool)
    .await
    .expect("stored targets")
}

#[tokio::test]
async fn dimensions_carry_by_endpoint_and_model_across_reorder_and_explicit_writes() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool).await;
    let app = worker_admin::router(fixture.state.clone());
    let endpoint = create_embeddings_endpoint(
        &app,
        &format!("{NAME_PREFIX}reviewer-carry-more"),
        "http://127.0.0.1:1",
    )
    .await;
    let rule_id = create_route(
        &app,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a", "embedding_dimensions_override": 512},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b", "embedding_dimensions_override": 1024}
        ]),
    )
    .await;

    // Reordered omitted update: each model keeps its own stored value.
    let (status, body) = patch_route(
        &app,
        rule_id,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b"},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a"}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "reordered update failed: {body}");
    assert_eq!(
        stored_dimensions(&fixture.pool, rule_id).await,
        vec![
            (Some("embedding-b".to_string()), Some(1024)),
            (Some("embedding-a".to_string()), Some(512)),
        ]
    );

    // An explicit value replaces only its own target; an explicit null clears.
    let (status, body) = patch_route(
        &app,
        rule_id,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b", "embedding_dimensions_override": 2048},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a", "embedding_dimensions_override": null}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "explicit update failed: {body}");
    assert_eq!(
        stored_dimensions(&fixture.pool, rule_id).await,
        vec![
            (Some("embedding-b".to_string()), Some(2048)),
            (Some("embedding-a".to_string()), None),
        ]
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint]).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_ambiguous_stored_identity_requires_an_explicit_dimension() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool).await;
    let app = worker_admin::router(fixture.state.clone());
    let endpoint = create_embeddings_endpoint(
        &app,
        &format!("{NAME_PREFIX}reviewer-ambiguous"),
        "http://127.0.0.1:1",
    )
    .await;
    let rule_id = create_route(
        &app,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a", "embedding_dimensions_override": 512},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-b", "embedding_dimensions_override": 1024},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-c", "embedding_dimensions_override": 128}
        ]),
    )
    .await;
    // Make two stored targets indistinguishable (same endpoint and model) with
    // different dimension values.
    sqlx::query(
        "UPDATE model_route_targets SET upstream_model = 'embedding-a' \
         WHERE rule_id = $1 AND upstream_model = 'embedding-b'",
    )
    .bind(rule_id)
    .execute(&fixture.pool)
    .await
    .expect("collapse two stored targets onto one identity");
    let before = stored_dimensions(&fixture.pool, rule_id).await;

    // The request itself has no duplicate, but the stored identity is
    // ambiguous, so an omitted value is refused.
    let (status, body) = patch_route(
        &app,
        rule_id,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a"},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-c"}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert_eq!(
        body["error"]["code"], "ambiguous_embedding_dimensions_override",
        "body: {body}"
    );
    // The refusal happens before any write.
    assert_eq!(stored_dimensions(&fixture.pool, rule_id).await, before);

    // An explicit value disambiguates and is stored.
    let (status, body) = patch_route(
        &app,
        rule_id,
        json!([
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-a", "embedding_dimensions_override": 256},
            {"endpoint_id": endpoint, "native_api": "embeddings", "upstream_model": "embedding-c"}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        stored_dimensions(&fixture.pool, rule_id).await,
        vec![
            (Some("embedding-a".to_string()), Some(256)),
            (Some("embedding-c".to_string()), Some(128)),
        ]
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint]).await;
    fixture.cleanup().await;
}

#[derive(Clone, Debug, Default)]
struct Recorded {
    path: String,
    body: Value,
    authorization: Option<String>,
}

#[derive(Clone)]
struct MockState {
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

async fn spawn_mock_upstream() -> (String, Arc<Mutex<Vec<Recorded>>>) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        recorded: recorded.clone(),
    };
    let app = Router::new()
        .route(
            "/v1/models",
            get(|State(state): State<MockState>| async move {
                state.recorded.lock().expect("recorded").push(Recorded {
                    path: "/v1/models".to_string(),
                    body: Value::Null,
                    authorization: None,
                });
                (
                    StatusCode::OK,
                    axum::Json(json!({"data": [{"id": "gpt-5"}]})),
                )
            }),
        )
        .route(
            "/v1/embeddings/models",
            get(|State(state): State<MockState>| async move {
                state.recorded.lock().expect("recorded").push(Recorded {
                    path: "/v1/embeddings/models".to_string(),
                    body: Value::Null,
                    authorization: None,
                });
                (
                    StatusCode::OK,
                    axum::Json(json!({"data": [{"id": EMBEDDING_MODEL}]})),
                )
            }),
        )
        .route(
            "/v1/embeddings",
            post(
                |State(state): State<MockState>,
                 headers: axum::http::HeaderMap,
                 axum::Json(body): axum::Json<Value>| async move {
                    state.recorded.lock().expect("recorded").push(Recorded {
                        path: "/v1/embeddings".to_string(),
                        body: body.clone(),
                        authorization: headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                    });
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "object": "list",
                            "model": body.get("model").cloned().unwrap_or(Value::Null),
                            "data": [{
                                "object": "embedding",
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3],
                            }],
                            "usage": {"prompt_tokens": 3, "total_tokens": 3},
                        })),
                    )
                },
            ),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), recorded)
}

#[tokio::test]
async fn the_embeddings_probe_uses_the_embeddings_catalog_and_forces_the_dimension() {
    let fixture = Fixture::open().await;
    cleanup_stale(&fixture.pool).await;
    let app = worker_admin::router(fixture.state.clone());
    let (base, recorded) = spawn_mock_upstream().await;

    let endpoint = create_embeddings_endpoint(&app, &format!("{NAME_PREFIX}probe"), &base).await;
    let rule_id = create_route(
        &app,
        json!([{
            "endpoint_id": endpoint,
            "enabled": true,
            "native_api": "embeddings",
            "upstream_model": EMBEDDING_MODEL,
            "embedding_dimensions_override": 3,
        }]),
    )
    .await;

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
    assert_eq!(status, StatusCode::OK, "route test failed: {body}");
    assert_eq!(body["passed"], 1, "the embeddings probe must pass: {body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    let message = results[0]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("embeddings; 1 vector(s), dimensions 3"),
        "unexpected probe message: {message}"
    );
    assert!(
        !message.contains("0.1"),
        "the probe must never echo a vector: {message}"
    );

    let recorded = recorded.lock().expect("recorded").clone();
    // A Generic endpoint lists its embeddings models at the OpenAI-compatible
    // `/v1/models` path (only OpenRouter has a separate embeddings catalog).
    assert!(
        recorded.iter().any(|r| r.path == "/v1/models"),
        "the endpoint model catalog must be listed: {recorded:?}"
    );
    let embedding_request = recorded
        .iter()
        .find(|r| r.path == "/v1/embeddings")
        .expect("an embeddings probe request");
    assert_eq!(embedding_request.body["model"], EMBEDDING_MODEL);
    assert_eq!(embedding_request.body["dimensions"], 3);
    assert_eq!(embedding_request.body["input"], "ping");
    assert!(
        embedding_request
            .authorization
            .as_deref()
            .is_some_and(|value| value.starts_with("Bearer ")),
        "the probe must authenticate with the endpoint key"
    );

    cleanup_rows(&fixture.pool, rule_id, &[endpoint]).await;
    fixture.cleanup().await;
}

// ---------------------------------------------------------------------------
// Issue #1032 P3 (test-only delta): the real public relay route.
//
// These tests drive the actual relay public router and a real worker over the
// bridge against a loopback upstream double. No database, provider account, or
// credential is involved: the relay uses its static client token and the worker
// its direct upstream configuration.
// ---------------------------------------------------------------------------

const EMBEDDINGS_BODY: &str = r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.125,0.25]}],"model":"qwen/qwen3-embedding-8b","usage":{"prompt_tokens":5,"total_tokens":5}}"#;

/// A loopback upstream that records the raw request line and body bytes and
/// answers with the fixed embeddings response.
async fn spawn_raw_recording_embeddings_upstream()
-> (SocketAddr, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let recorded = Arc::new(Mutex::new(Vec::new()));
    let log = recorded.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = vec![0u8; 65536];
                let read = sock.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_string();
                log.lock()
                    .expect("recorded")
                    .push((path, body.as_bytes().to_vec()));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    EMBEDDINGS_BODY.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.write_all(EMBEDDINGS_BODY.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (address, recorded)
}

async fn spawn_relay_with_token(client_token: &str) -> (SocketAddr, SocketAddr, RelayHandle) {
    let public_listener = TcpListener::bind("127.0.0.1:0").await.expect("public bind");
    let public_addr = public_listener.local_addr().expect("public address");
    let worker_listener = TcpListener::bind("127.0.0.1:0").await.expect("worker bind");
    let worker_addr = worker_listener.local_addr().expect("worker address");
    let (public_app, worker_app, handle) = relay::apps(RelayConfig {
        bind: public_addr.to_string(),
        worker_bind: worker_addr.to_string(),
        client_token: client_token.to_string(),
        worker_token: "worker-token".to_string(),
        request_timeout_seconds: 10,
        worker_heartbeat_timeout_seconds: 30,
        bridge_encryption_mode: BridgeEncryptionMode::Off,
        ..RelayConfig::default()
    });
    tokio::spawn(async move {
        let _ = axum::serve(
            public_listener,
            public_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await;
    });
    tokio::spawn(async move {
        let _ = axum::serve(
            worker_listener,
            worker_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await;
    });
    (public_addr, worker_addr, handle)
}

async fn wait_for_registered_worker(
    handle: &RelayHandle,
    worker_handle: &mut tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    for _ in 0..200 {
        if handle.worker_count().await > 0 {
            return;
        }
        tokio::select! {
            result = &mut *worker_handle => panic!("worker exited before connecting: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }
    panic!("worker did not connect");
}

#[tokio::test]
async fn the_public_embeddings_route_requires_a_client_key_and_then_forwards() {
    let (upstream_addr, recorded) = spawn_raw_recording_embeddings_upstream().await;
    let (relay_public, relay_worker, handle) = spawn_relay_with_token("client-token").await;
    let worker_config = WorkerConfig {
        relay_urls: vec![format!("ws://{relay_worker}/ws/worker")],
        worker_token: "worker-token".to_string(),
        upstream_base_url: format!("http://{upstream_addr}"),
        upstream_api_key: "upstream-key".to_string(),
        // Issue #1032: an embeddings-native target serves `/v1/embeddings`.
        upstream_native_api: NativeApi::Embeddings,
        connect_timeout_seconds: 5,
        ..WorkerConfig::default()
    };
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });
    wait_for_registered_worker(&handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    // A no-match input is forwarded byte-for-byte; there is no rewrite.
    let body = r#"{"model":"qwen/qwen3-embedding-8b","input":"plain text with no redaction match","encoding_format":"float"}"#;

    // Unauthorized: the relay rejects before any worker or upstream contact.
    let response = client
        .post(format!("http://{relay_public}/v1/embeddings"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .expect("unauthorized request");
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "an unauthenticated embeddings request must be rejected"
    );
    assert!(
        recorded.lock().expect("recorded").is_empty(),
        "an unauthorized request must never reach the upstream"
    );

    // Authorized: the request reaches the real upstream and the body is relayed.
    let response = client
        .post(format!("http://{relay_public}/v1/embeddings"))
        .bearer_auth("client-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .expect("authorized request");
    assert_eq!(response.status(), StatusCode::OK);
    let relayed = response.text().await.expect("relayed body");
    assert_eq!(
        relayed, EMBEDDINGS_BODY,
        "the embeddings response must be relayed unchanged"
    );

    let requests = recorded.lock().expect("recorded").clone();
    assert_eq!(requests.len(), 1, "exactly one upstream request");
    assert_eq!(requests[0].0, "/v1/embeddings");
    assert_eq!(
        String::from_utf8_lossy(&requests[0].1),
        body,
        "a no-match input must be forwarded byte-for-byte"
    );

    worker_handle.abort();
    let _ = worker_handle.await;
}
