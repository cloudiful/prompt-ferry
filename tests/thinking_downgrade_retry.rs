//! Issue #556: thinking-400 handling end-to-end.
//!
//! A mock upstream rejects the first tool-bearing turn with the reasoning-echo
//! fingerprint (`... must be passed back`). Ferry must resend that one turn
//! with thinking off, keep every non-fingerprint error untouched, and return
//! the original upstream error when the resend is rejected again. The last
//! case covers the pre-flight downgrade: a parent turn whose artifact proves
//! it produced no reasoning downgrades the next turn before the first attempt.
//!
//! Database-backed like the other worker integration tests; skipped when
//! `PROMPT_FERRY_TEST_DATABASE_URL` is unset.

#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/prompt_logging_harness.rs"]
mod prompt_logging_harness;
#[path = "support/replay_harness.rs"]
mod relay_harness;
#[path = "support/worker_database_url_harness.rs"]
mod worker_database_url_harness;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, Uri, header},
    response::Response,
    routing::post,
};
use prompt_ferry::{config::NativeApi, config::NativeApiSource, db};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::prompt_logging_harness::enable_prompt_logging;
use crate::relay_harness::{spawn_relay, wait_for_worker, worker_config};
use crate::worker_database_url_harness::worker_database_url;

const FINGERPRINT_MESSAGE: &str =
    "Error code: 400 - reasoning_content in the thinking mode must be passed back to the model.";
const OTHER_MESSAGE: &str = "invalid parameter: unsupported field";
const THINKING_MODEL: &str = "thinking-test";
const DISABLE_ENV: &str = "PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE";

/// Serializes the env-var case against every other case in this file, because
/// the escape hatch is process-global and the worker reads it per request.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamMode {
    /// 200 for every request.
    AlwaysOk,
    /// Fingerprint 400 on the first request, 200 afterwards.
    FingerprintFirstThenOk,
    /// 400 without the fingerprint for every request.
    OtherBadRequest,
    /// Fingerprint 400 for every request.
    AlwaysFingerprint,
}

struct UpstreamState {
    mode: UpstreamMode,
    /// First request answers with an assistant tool call and no reasoning.
    tool_call_first: bool,
    bodies: Mutex<Vec<Value>>,
}

impl UpstreamState {
    async fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().await.clone()
    }
}

fn tools() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {}},
        }
    }])
}

async fn spawn_upstream(
    mode: UpstreamMode,
    tool_call_first: bool,
) -> (SocketAddr, Arc<UpstreamState>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(UpstreamState {
        mode,
        tool_call_first,
        bodies: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_completion))
        .route("/v1/responses", post(fake_completion))
        .with_state(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, state)
}

async fn fake_completion(
    State(state): State<Arc<UpstreamState>>,
    uri: Uri,
    body: Bytes,
) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let index = {
        let mut bodies = state.bodies.lock().await;
        bodies.push(value.clone());
        bodies.len()
    };
    let reject = match state.mode {
        UpstreamMode::AlwaysOk => false,
        UpstreamMode::FingerprintFirstThenOk => index == 1,
        UpstreamMode::OtherBadRequest | UpstreamMode::AlwaysFingerprint => true,
    };
    if reject {
        let message = if state.mode == UpstreamMode::OtherBadRequest {
            OTHER_MESSAGE
        } else {
            FINGERPRINT_MESSAGE
        };
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"error": {"message": message, "type": "invalid_request_error"}}),
        );
    }
    if uri.path().ends_with("/responses") {
        return json_response(
            StatusCode::OK,
            json!({
                "id": "resp_1",
                "object": "response",
                "created_at": 123,
                "status": "completed",
                "model": THINKING_MODEL,
                "output": [{
                    "id": "msg_1",
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "done"}]
                }],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
            }),
        );
    }
    if state.tool_call_first && index == 1 {
        return json_response(
            StatusCode::OK,
            json!({
                "id": "chatcmpl_turn1",
                "created": 123,
                "model": THINKING_MODEL,
                "choices": [{
                    "message": {
                        "content": Value::Null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "get_weather", "arguments": "{\"city\":\"Boston\"}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
            }),
        );
    }
    json_response(
        StatusCode::OK,
        json!({
            "id": format!("chatcmpl_{index}"),
            "created": 124,
            "model": THINKING_MODEL,
            "choices": [{
                "message": {"content": "done"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 2, "total_tokens": 6},
        }),
    )
}

fn json_response(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

struct ThinkingHarness {
    schema: TestSchema,
    relay_addr: SocketAddr,
    upstream: Arc<UpstreamState>,
    worker_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl ThinkingHarness {
    async fn spawn(
        native_api: NativeApi,
        mode: UpstreamMode,
        tool_call_first: bool,
    ) -> anyhow::Result<Self> {
        let schema = TestSchema::new().await?;
        enable_prompt_logging(&schema).await?;
        let (upstream_addr, upstream) = spawn_upstream(mode, tool_call_first).await;
        let endpoint = db::create_endpoint(
            &schema.pool,
            db::EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "thinking-downgrade-upstream".to_string(),
                provider: db::EndpointProvider::Generic,
                provider_region: None,
                service_tier: Default::default(),
                base_url: format!("http://{upstream_addr}"),
                native_api,
                native_api_source: NativeApiSource::Manual,
                api_key: "upstream-key".to_string(),
                api_keys: vec![],
                key_lb_enabled: false,
                enabled: true,
                proxy_url: None,
                active_windows: None,
            },
        )
        .await?;
        // The target effort stands in for a configured `teo` override: the
        // downgraded turn must drop it instead of letting it re-enable
        // thinking.
        db::create_model_endpoint_rule(
            &schema.pool,
            db::ModelEndpointRuleCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                model_pattern: "*".to_string(),
                routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
                enabled: true,
                targets: vec![db::ModelRouteTargetCreate {
                    endpoint_id: endpoint.endpoint_id,
                    enabled: true,
                    upstream_model: None,
                    native_api: NativeApi::Auto,
                    proxy_url_override: None,
                    active_windows: None,
                    dev_system_normalize: false,
                    thinking_effort_override: Some("high".to_string()),
                    compact_mode: db::CompactMode::Passthrough,
                }],
            },
        )
        .await?;

        let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
        let config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
        let mut worker_handle = tokio::spawn(async move {
            prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
        });
        wait_for_worker(&relay_handle, &mut worker_handle).await;
        Ok(Self {
            schema,
            relay_addr,
            upstream,
            worker_handle,
        })
    }

    async fn post_chat(&self, session_id: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("http://{}/v1/chat/completions", self.relay_addr))
            .bearer_auth("client-token")
            .header("X-Session-Id", session_id)
            .json(&json!({
                "model": THINKING_MODEL,
                "stream": false,
                "thinking": {"type": "enabled"},
                "tools": tools(),
                "messages": [{"role": "user", "content": "hi"}],
            }))
            .send()
            .await
            .expect("chat request should send")
    }

    async fn post_responses(&self, session_id: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("http://{}/v1/responses", self.relay_addr))
            .bearer_auth("client-token")
            .header("X-Session-Id", session_id)
            .json(&json!({
                "model": THINKING_MODEL,
                "stream": false,
                "reasoning": {"effort": "low"},
                "tools": tools(),
                "input": [{"type": "message", "role": "user", "content": "hi"}],
            }))
            .send()
            .await
            .expect("responses request should send")
    }

    async fn shutdown(self) -> anyhow::Result<()> {
        self.worker_handle.abort();
        wait_for_schema_quiescent(&self.schema).await?;
        self.schema.cleanup().await
    }
}

impl Drop for ThinkingHarness {
    fn drop(&mut self) {
        self.worker_handle.abort();
    }
}

async fn wait_for_schema_quiescent(schema: &TestSchema) -> anyhow::Result<()> {
    for _ in 0..400 {
        let locks = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM pg_locks locks
            JOIN pg_class cls ON cls.oid = locks.relation
            JOIN pg_namespace nsp ON nsp.oid = cls.relnamespace
            WHERE nsp.nspname = $1
              AND locks.pid <> pg_backend_pid()
            "#,
        )
        .bind(&schema.schema_name)
        .fetch_one(&schema.pool)
        .await?;
        if locks == 0 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    anyhow::bail!("schema did not quiesce before cleanup")
}

async fn wait_for_assistant_artifact(schema: &TestSchema) -> anyhow::Result<bool> {
    for _ in 0..200 {
        let row = sqlx::query_scalar::<_, bool>(
            "SELECT has_reasoning_content FROM request_record_assistant_artifacts ORDER BY event_id LIMIT 1",
        )
        .fetch_optional(&schema.pool)
        .await?;
        if let Some(has_reasoning_content) = row {
            return Ok(has_reasoning_content);
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    anyhow::bail!("assistant artifact was not persisted")
}

impl Drop for DisableThinkingDowngrade {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(DISABLE_ENV) };
    }
}

struct DisableThinkingDowngrade;

#[tokio::test]
async fn chat_fingerprint_400_retries_once_with_thinking_disabled() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    let harness =
        ThinkingHarness::spawn(NativeApi::Chat, UpstreamMode::FingerprintFirstThenOk, false)
            .await?;

    let response = harness.post_chat("chat-fingerprint-retry").await;
    assert_eq!(response.status(), StatusCode::OK);
    let bodies = harness.upstream.bodies().await;
    assert_eq!(bodies.len(), 2, "non-success attempt must be resent once");
    // First attempt: the target effort override and the caller thinking switch
    // both went out untouched.
    assert_eq!(bodies[0]["thinking"]["type"], "enabled");
    assert_eq!(bodies[0]["reasoning_effort"], "high");
    // Retry: thinking off and the effort override dropped.
    assert_eq!(bodies[1]["thinking"]["type"], "disabled");
    assert!(bodies[1].get("reasoning_effort").is_none());

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn responses_fingerprint_400_retries_once_with_effort_none() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    let harness = ThinkingHarness::spawn(
        NativeApi::Responses,
        UpstreamMode::FingerprintFirstThenOk,
        false,
    )
    .await?;

    let response = harness.post_responses("responses-fingerprint-retry").await;
    assert_eq!(response.status(), StatusCode::OK);
    let bodies = harness.upstream.bodies().await;
    assert_eq!(bodies.len(), 2, "non-success attempt must be resent once");
    assert_eq!(bodies[0]["reasoning"]["effort"], "high");
    assert_eq!(bodies[1]["reasoning"]["effort"], "none");

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn non_fingerprint_400_is_not_retried() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    let harness =
        ThinkingHarness::spawn(NativeApi::Chat, UpstreamMode::OtherBadRequest, false).await?;

    let response = harness.post_chat("chat-other-400").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bodies = harness.upstream.bodies().await;
    assert_eq!(bodies.len(), 1, "an unrelated 400 must not be resent");
    assert_eq!(bodies[0]["thinking"]["type"], "enabled");

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn fingerprint_400_after_retry_returns_the_original_error() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    let harness =
        ThinkingHarness::spawn(NativeApi::Chat, UpstreamMode::AlwaysFingerprint, false).await?;

    let response = harness.post_chat("chat-original-error").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await?;
    assert!(
        body.contains("must be passed back"),
        "original upstream error must surface: {body}"
    );
    let bodies = harness.upstream.bodies().await;
    assert_eq!(bodies.len(), 2, "the turn is retried exactly once");
    assert_eq!(bodies[1]["thinking"]["type"], "disabled");

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn env_bypass_disables_the_fingerprint_retry() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    unsafe { std::env::set_var(DISABLE_ENV, "1") };
    let _bypass = DisableThinkingDowngrade;
    let harness =
        ThinkingHarness::spawn(NativeApi::Chat, UpstreamMode::AlwaysFingerprint, false).await?;

    let response = harness.post_chat("chat-env-bypass").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await?;
    assert!(body.contains("must be passed back"), "got {body}");
    let bodies = harness.upstream.bodies().await;
    assert_eq!(
        bodies.len(),
        1,
        "the bypass keeps the pre-fix single attempt"
    );
    assert_eq!(bodies[0]["thinking"]["type"], "enabled");

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn parent_turn_without_reasoning_downgrades_before_the_first_attempt() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let _lock = ENV_LOCK.lock().await;
    let harness = ThinkingHarness::spawn(NativeApi::Chat, UpstreamMode::AlwaysOk, true).await?;

    // Turn 1 answers with a tool call and no reasoning content, so the stored
    // artifact proves the parent turn has nothing to pass back.
    let first = harness.post_chat("thinking-thread").await;
    assert_eq!(first.status(), StatusCode::OK);
    assert!(
        !wait_for_assistant_artifact(&harness.schema).await?,
        "the mock tool-call turn carries no reasoning content"
    );
    // Turn 2 continues the same conversation: the pre-flight downgrade must
    // apply to the very first upstream attempt, without any 400 round trip.
    let second = harness.post_chat("thinking-thread").await;
    assert_eq!(second.status(), StatusCode::OK);
    let bodies = harness.upstream.bodies().await;
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["thinking"]["type"], "enabled");
    assert_eq!(bodies[1]["thinking"]["type"], "disabled");
    assert!(bodies[1].get("reasoning_effort").is_none());

    harness.shutdown().await?;
    Ok(())
}
