//! Issue #993 P1: the admin endpoint model listing.
//!
//! The listing is an upstream discovery read: it follows the endpoint's own
//! base URL, proxy, and protocol auth instead of a provider allowlist. These
//! tests drive it through the SQLite-backed admin router against a mock
//! upstream, so no production service, credential, or database is involved.

#[path = "support/endpoint_create_fixture.rs"]
mod fixture;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use prompt_ferry::worker_admin;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

use crate::fixture::{Fixture, json_body, request};

#[derive(Clone)]
enum Upstream {
    /// Answers with a model listing built from the given identifiers.
    Models(Vec<String>),
    /// Answers with an error status and a body the admin API must not echo.
    Failure(StatusCode),
    /// Answers with a body that is not a model listing.
    Unreadable,
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    api_key: Option<String>,
    anthropic_version: Option<String>,
}

#[derive(Clone)]
struct MockState {
    upstream: Upstream,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
}

struct MockUpstream {
    base_url: String,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl MockUpstream {
    fn requests(&self) -> Vec<RecordedRequest> {
        self.recorded.lock().expect("recorded requests").clone()
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn models_handler(State(state): State<MockState>, uri: Uri, headers: HeaderMap) -> Response {
    state
        .recorded
        .lock()
        .expect("recorded requests")
        .push(RecordedRequest {
            path: uri.path().to_string(),
            authorization: header_value(&headers, header::AUTHORIZATION.as_str()),
            api_key: header_value(&headers, "x-api-key"),
            anthropic_version: header_value(&headers, "anthropic-version"),
        });
    match &state.upstream {
        Upstream::Models(ids) => {
            let data: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
            (
                StatusCode::OK,
                axum::Json(json!({ "object": "list", "data": data })),
            )
                .into_response()
        }
        Upstream::Failure(status) => (*status, "upstream failure detail").into_response(),
        Upstream::Unreadable => (StatusCode::OK, "not-a-model-listing").into_response(),
    }
}

async fn spawn_mock_upstream(upstream: Upstream) -> anyhow::Result<MockUpstream> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/models", get(models_handler))
        .with_state(MockState {
            upstream,
            recorded: recorded.clone(),
        });
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(MockUpstream {
        base_url: format!("http://{address}"),
        recorded,
    })
}

/// Create an endpoint that points at the mock upstream.
async fn create_endpoint(
    app: &Router,
    base_url: &str,
    api_key: &str,
    native_api: &str,
) -> anyhow::Result<Uuid> {
    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/endpoints".to_string(),
            Some(json!({
                "scope": "admin",
                "owner_user_id": null,
                "name": "mock-upstream",
                "provider": "generic",
                "provider_region": null,
                "plan": null,
                "service_tier": null,
                "base_url": base_url,
                "api_key": api_key,
                "api_keys": [],
                "key_lb_enabled": false,
                "protocol_mode": "manual",
                "native_api_override": native_api,
                "enabled": true,
                "mcp_enabled": false,
            })),
        ))
        .await?;
    assert_eq!(
        created.status(),
        StatusCode::OK,
        "the mock endpoint must be created"
    );
    let created_body = json_body(created).await;
    Ok(created_body["endpoint_id"]
        .as_str()
        .expect("created endpoint id")
        .parse::<Uuid>()?)
}

fn models_request(endpoint_id: Uuid) -> Request<Body> {
    request(
        "GET",
        format!("/api/v1/admin/endpoints/{endpoint_id}/models"),
        None,
    )
}

/// Issue #1032: the optional protocol selector scopes the catalog listing.
fn models_request_with_selector(endpoint_id: Uuid, native_api: &str) -> Request<Body> {
    request(
        "GET",
        format!("/api/v1/admin/endpoints/{endpoint_id}/models?native_api={native_api}"),
        None,
    )
}

// Issue #1032: the listing cache is keyed by the effective protocol, so a
/// chat listing never satisfies an embeddings ask for the same endpoint, while
/// a repeat of the same ask is served from the cache.
#[tokio::test]
async fn the_catalog_selector_scopes_the_listing_cache_per_protocol() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(Upstream::Models(vec!["gpt-4o".to_string()])).await?;
    let endpoint_id = create_endpoint(&app, &upstream.base_url, "sk-mock", "chat").await?;

    // The endpoint's own protocol (chat) lists and caches once.
    let response = app.clone().oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["models"], json!(["gpt-4o"]));
    assert_eq!(upstream.requests().len(), 1);

    let response = app.clone().oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["models"], json!(["gpt-4o"]));
    assert_eq!(
        upstream.requests().len(),
        1,
        "a repeat of the same catalog must be served from the cache"
    );

    // An explicit embeddings selector is a distinct cache entry, so it reads
    // the upstream again instead of reusing the chat answer.
    let response = app
        .clone()
        .oneshot(models_request_with_selector(endpoint_id, "embeddings"))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["models"], json!(["gpt-4o"]));
    assert_eq!(
        upstream.requests().len(),
        2,
        "a protocol-scoped listing must not reuse another protocol's cache entry"
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn lists_the_sorted_unique_model_ids_of_the_endpoint() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(Upstream::Models(vec![
        "gpt-4o-mini".to_string(),
        "gpt-4o".to_string(),
        "gpt-4o-mini".to_string(),
    ]))
    .await?;
    let endpoint_id = create_endpoint(&app, &upstream.base_url, "sk-mock", "chat").await?;

    let response = app.clone().oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["models"], json!(["gpt-4o", "gpt-4o-mini"]));

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].path, "/v1/models");
    assert_eq!(recorded[0].authorization.as_deref(), Some("Bearer sk-mock"));
    assert!(recorded[0].api_key.is_none());

    // A repeated read inside the cache lifetime never re-queries the upstream.
    let repeated = app.oneshot(models_request(endpoint_id)).await?;
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated_body = json_body(repeated).await;
    assert_eq!(repeated_body["models"], json!(["gpt-4o", "gpt-4o-mini"]));
    assert_eq!(upstream.requests().len(), 1);

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn an_anthropic_endpoint_authenticates_with_its_own_protocol() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());
    let upstream =
        spawn_mock_upstream(Upstream::Models(vec!["claude-sonnet-4".to_string()])).await?;
    let endpoint_id = create_endpoint(
        &app,
        &upstream.base_url,
        "sk-anthropic",
        "anthropic_messages",
    )
    .await?;

    let response = app.clone().oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["models"], json!(["claude-sonnet-4"]));

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].api_key.as_deref(), Some("sk-anthropic"));
    assert_eq!(recorded[0].anthropic_version.as_deref(), Some("2023-06-01"));
    assert!(recorded[0].authorization.is_none());

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn a_failing_upstream_is_a_bounded_admin_error() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(Upstream::Failure(StatusCode::UNAUTHORIZED)).await?;
    let endpoint_id = create_endpoint(&app, &upstream.base_url, "sk-secret", "chat").await?;

    let response = app.oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "model_discovery_failed");
    let rendered = body.to_string();
    assert!(
        !rendered.contains("upstream failure detail"),
        "the upstream body must never reach the admin API: {rendered}"
    );
    assert!(
        !rendered.contains("sk-secret"),
        "the endpoint credential must never reach the admin API: {rendered}"
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn an_unreadable_upstream_body_is_a_bounded_admin_error() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());
    let upstream = spawn_mock_upstream(Upstream::Unreadable).await?;
    let endpoint_id = create_endpoint(&app, &upstream.base_url, "sk-mock", "chat").await?;

    let response = app.oneshot(models_request(endpoint_id)).await?;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "model_discovery_failed");
    assert!(!body.to_string().contains("not-a-model-listing"));

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn a_missing_endpoint_is_not_found() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());

    let response = app.oneshot(models_request(Uuid::new_v4())).await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "not_found");

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn the_listing_requires_an_admin_session() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());

    let anonymous = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/admin/endpoints/{}/models", Uuid::new_v4()))
        .body(Body::empty())?;
    let response = app.oneshot(anonymous).await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    fixture.cleanup().await;
    Ok(())
}
