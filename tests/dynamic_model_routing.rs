#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/replay_harness.rs"]
mod relay_harness;
#[path = "support/worker_database_url_harness.rs"]
mod worker_database_url_harness;

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use prompt_ferry::{config::NativeApiSource, db};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::relay_harness::{spawn_relay, wait_for_worker, worker_config};
use crate::worker_database_url_harness::worker_database_url;

#[derive(Clone)]
struct UpstreamState {
    model_ids: Vec<String>,
    label: &'static str,
    bodies: Arc<Mutex<Vec<Value>>>,
}

async fn fake_models(State(state): State<UpstreamState>) -> Response {
    let data = state
        .model_ids
        .iter()
        .map(|model| serde_json::json!({ "id": model }))
        .collect::<Vec<_>>();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "object": "list", "data": data }).to_string(),
        ))
        .unwrap()
}

async fn fake_chat_completion(State(state): State<UpstreamState>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    state.bodies.lock().await.push(value.clone());
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let payload = serde_json::json!({
        "id": format!("chatcmpl_{}", state.label),
        "object": "chat.completion",
        "created": 123,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": format!("served-by-{}", state.label),
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 2,
            "completion_tokens": 2,
            "total_tokens": 4
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

async fn spawn_upstream(
    label: &'static str,
    model_ids: &[&str],
) -> (std::net::SocketAddr, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let state = UpstreamState {
        model_ids: model_ids.iter().map(|model| (*model).to_string()).collect(),
        label,
        bodies: bodies.clone(),
    };
    let app = Router::new()
        .route("/v1/models", get(fake_models))
        .route("/v1/chat/completions", post(fake_chat_completion))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, bodies)
}

#[tokio::test]
async fn auto_discovers_endpoint_for_model_when_whitelist_is_disabled() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping dynamic routing test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    db::set_bool_setting(&schema.pool, "model_route_whitelist_enabled", false).await?;

    let (beta_addr, beta_bodies) = spawn_upstream("beta", &["beta-model"]).await;
    let (alpha_addr, alpha_bodies) = spawn_upstream("alpha", &["alpha-model"]).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;

    db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "beta".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{beta_addr}"),
            native_api: prompt_ferry::config::NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "alpha".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{alpha_addr}"),
            native_api: prompt_ferry::config::NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;

    let config = worker_config(worker_addr, alpha_addr, &worker_database_url(&schema)?);
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "beta-model",
            "stream": false,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await?;
    assert!(body.contains("served-by-beta"));

    assert!(alpha_bodies.lock().await.is_empty());
    let beta_requests = beta_bodies.lock().await;
    assert_eq!(beta_requests.len(), 1);
    assert_eq!(beta_requests[0]["model"].as_str(), Some("beta-model"));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}
