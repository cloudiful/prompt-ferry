//! Issue #599 R2b: ChatGPT OAuth protocol client against a local mock issuer.
//!
//! The client is exercised exactly as the admin handlers use it (device-code
//! start/poll, code exchange, refresh) so the upstream contract and the error
//! classification are pinned without touching the real ChatGPT endpoints.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use axum::{Json, Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{Duration as ChronoDuration, Utc};
use prompt_ferry::worker_admin::oauth::{
    ChatgptOAuthError, DevicePollOutcome, authorize_url, exchange_authorization_code,
    generate_pkce, parse_redirect_url, poll_device_authorization, random_state,
    refresh_chatgpt_tokens, request_device_authorization,
};
use prompt_ferry::{
    config::{NativeApi, NativeApiSource as DbNativeApiSource},
    db::{self, ConfigRepository, EndpointCreate, EndpointOAuthTokenSet, EndpointProvider},
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    standalone_config::StandaloneConfigStore,
    worker_admin,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};
use prompt_ferry_runtime_env::relay_secrets::RelaySecretManager;
use reqwest::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

type PollCounter = Arc<Mutex<u32>>;

async fn spawn_issuer(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock issuer");
    let address = listener.local_addr().expect("mock issuer address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve mock issuer");
    });
    format!("http://{address}")
}

fn poll_counter() -> PollCounter {
    Arc::new(Mutex::new(0))
}

async fn usercode() -> Json<Value> {
    Json(serde_json::json!({
        "device_auth_id": "device-auth-1",
        "user_code": "ABCD-1234",
        "interval": "1",
    }))
}

async fn usercode_rate_limited() -> (StatusCode, Json<Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({ "error": { "message": "slow down" } })),
    )
}

async fn device_token_pending_then_authorized(
    State(polls): State<PollCounter>,
) -> (StatusCode, Json<Value>) {
    let mut polls = polls.lock().expect("mock poll counter");
    *polls += 1;
    if *polls == 1 {
        // ChatGPT keeps answering 403 until the operator confirms the code.
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({})));
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "authorization_code": "auth-code-1",
            "code_verifier": "device-verifier-1",
        })),
    )
}

async fn device_token_denied() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": { "message": "device code expired" } })),
    )
}

async fn device_token_server_error() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({})),
    )
}

async fn oauth_token(body: Bytes) -> (StatusCode, Json<Value>) {
    let body = String::from_utf8_lossy(&body);
    let params = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect::<HashMap<_, _>>();
    if params.get("refresh_token").copied() == Some("revoked-refresh") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "refresh token revoked",
            })),
        );
    }
    if params.get("refresh_token").copied() == Some("limited-refresh") {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": { "message": "rate limited" } })),
        );
    }
    if params.get("code").copied() == Some("bad-code") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": { "code": "invalid_grant", "message": "code expired" },
            })),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": "access-1",
            "refresh_token": "refresh-1",
            "expires_in": 3600,
        })),
    )
}

#[tokio::test]
async fn device_login_polls_pending_then_exchanges_tokens() {
    let router = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(usercode))
        .route(
            "/api/accounts/deviceauth/token",
            post(device_token_pending_then_authorized),
        )
        .route("/oauth/token", post(oauth_token))
        .with_state(poll_counter());
    let issuer = spawn_issuer(router).await;
    let client = Client::new();

    let device = request_device_authorization(&client, &issuer)
        .await
        .expect("device start");
    assert_eq!(device.device_auth_id, "device-auth-1");
    assert_eq!(device.user_code, "ABCD-1234");
    assert_eq!(device.interval_seconds, 1);

    assert!(
        matches!(
            poll_device_authorization(&client, &issuer, &device)
                .await
                .expect("first poll"),
            DevicePollOutcome::Pending
        ),
        "the first poll must stay pending"
    );
    let DevicePollOutcome::Authorized {
        authorization_code,
        code_verifier,
    } = poll_device_authorization(&client, &issuer, &device)
        .await
        .expect("second poll")
    else {
        panic!("the second poll must be authorized");
    };
    assert_eq!(authorization_code, "auth-code-1");

    let tokens = exchange_authorization_code(
        &client,
        &issuer,
        &authorization_code,
        &format!("{issuer}/deviceauth/callback"),
        &code_verifier,
    )
    .await
    .expect("code exchange");
    assert_eq!(tokens.access_token, "access-1");
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
    assert_eq!(tokens.expires_in_seconds, Some(3600));
}

#[tokio::test]
async fn device_and_grant_errors_are_classified() {
    let client = Client::new();

    let router = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(usercode))
        .route("/api/accounts/deviceauth/token", post(device_token_denied))
        .with_state(poll_counter());
    let issuer = spawn_issuer(router).await;
    let device = request_device_authorization(&client, &issuer)
        .await
        .expect("device start");
    match poll_device_authorization(&client, &issuer, &device)
        .await
        .expect("denied poll")
    {
        DevicePollOutcome::Denied(message) => assert_eq!(message, "device code expired"),
        _ => panic!("a rejected device code must be denied"),
    }

    let router = Router::new().route(
        "/api/accounts/deviceauth/usercode",
        post(usercode_rate_limited),
    );
    let issuer = spawn_issuer(router).await;
    assert!(
        matches!(
            request_device_authorization(&client, &issuer).await,
            Err(ChatgptOAuthError::RateLimited(_))
        ),
        "a 429 device start must surface as rate limited"
    );

    let router = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(usercode))
        .route(
            "/api/accounts/deviceauth/token",
            post(device_token_server_error),
        )
        .with_state(poll_counter());
    let issuer = spawn_issuer(router).await;
    let device = request_device_authorization(&client, &issuer)
        .await
        .expect("device start");
    assert!(
        matches!(
            poll_device_authorization(&client, &issuer, &device).await,
            Err(ChatgptOAuthError::Upstream {
                status: Some(500),
                ..
            })
        ),
        "a 5xx poll must stay a retryable upstream error"
    );
}

#[tokio::test]
async fn token_endpoint_grant_errors_are_classified() {
    let router = Router::new().route("/oauth/token", post(oauth_token));
    let issuer = spawn_issuer(router).await;
    let client = Client::new();

    let refreshed = refresh_chatgpt_tokens(&client, &issuer, "refresh-1")
        .await
        .expect("successful refresh");
    assert_eq!(refreshed.access_token, "access-1");
    assert_eq!(refreshed.refresh_token.as_deref(), Some("refresh-1"));

    match refresh_chatgpt_tokens(&client, &issuer, "revoked-refresh").await {
        Err(ChatgptOAuthError::InvalidGrant(message)) => {
            assert_eq!(message, "refresh token revoked");
        }
        _ => panic!("a revoked refresh token must classify as invalid_grant"),
    }
    assert!(
        matches!(
            refresh_chatgpt_tokens(&client, &issuer, "limited-refresh").await,
            Err(ChatgptOAuthError::RateLimited(_))
        ),
        "a 429 refresh must surface as rate limited"
    );
    assert!(
        matches!(
            exchange_authorization_code(
                &client,
                &issuer,
                "bad-code",
                "http://localhost:1455/auth/callback",
                "verifier-1",
            )
            .await,
            Err(ChatgptOAuthError::InvalidGrant(_))
        ),
        "a rejected authorization code must classify as invalid_grant"
    );
}

#[test]
fn authorize_url_mirrors_the_codex_client_contract() {
    let pkce = generate_pkce();
    let state = random_state();
    let url = authorize_url(
        "https://issuer.example.test",
        "http://localhost:1455/auth/callback",
        &pkce,
        &state,
    );
    let parsed = reqwest::Url::parse(&url).expect("authorize URL");
    assert_eq!(parsed.path(), "/oauth/authorize");
    let params = parsed.query_pairs().collect::<HashMap<_, _>>();
    for (key, expected) in [
        ("client_id", "app_EMoamEEZ73f0CkXaXp7hrann"),
        ("redirect_uri", "http://localhost:1455/auth/callback"),
        ("scope", "openid profile email offline_access"),
        ("code_challenge_method", "S256"),
        ("codex_cli_simplified_flow", "true"),
    ] {
        assert_eq!(
            params.get(key).map(|value| value.as_ref()),
            Some(expected),
            "parameter {key}"
        );
    }
    assert_eq!(
        params.get("code_challenge").map(|value| value.as_ref()),
        Some(pkce.challenge.as_str())
    );
    assert_eq!(
        params.get("state").map(|value| value.as_ref()),
        Some(state.as_str())
    );
}

#[test]
fn pkce_and_state_material_is_rfc7636_shaped() {
    let pkce = generate_pkce();
    assert_eq!(pkce.verifier.len(), 43);
    assert!(
        pkce.verifier
            .chars()
            .all(|ch| { ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~') })
    );
    let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
    assert_eq!(pkce.challenge, expected);
    let state = random_state();
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(state.as_bytes())
            .expect("state is base64url")
            .len(),
        32
    );
}

#[test]
fn redirect_url_parsing_accepts_pasted_forms_and_rejects_mismatches() {
    let state = "state-123";
    assert_eq!(
        parse_redirect_url(
            "http://localhost:1455/auth/callback?code=code-1&state=state-123",
            state,
        )
        .expect("full redirect URL"),
        "code-1"
    );
    assert_eq!(
        parse_redirect_url("code=code-2&state=state-123", state).expect("bare query string"),
        "code-2"
    );
    assert!(matches!(
        parse_redirect_url(
            "http://localhost:1455/auth/callback?code=code-1&state=other",
            state,
        ),
        Err(ChatgptOAuthError::InvalidRedirect(_))
    ));
    assert!(matches!(
        parse_redirect_url("http://localhost:1455/auth/callback?state=state-123", state),
        Err(ChatgptOAuthError::InvalidRedirect(_))
    ));
    match parse_redirect_url(
        "http://localhost:1455/auth/callback?error=access_denied&state=state-123",
        state,
    ) {
        Err(ChatgptOAuthError::AuthorizationDenied(message)) => {
            assert_eq!(message, "access_denied");
        }
        _ => panic!("an error redirect must be denied"),
    }
}

// ---------------------------------------------------------------------------
// Route-level integration tests: the real admin router over SQLite state with
// the ChatGPT issuer pointed at a local mock.
// ---------------------------------------------------------------------------

static ROUTE_ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

#[derive(Clone, Default)]
struct RouteIssuerState {
    device_polls: Arc<Mutex<HashMap<String, u32>>>,
    device_seq: Arc<AtomicU32>,
}

async fn route_usercode(State(state): State<RouteIssuerState>) -> Json<Value> {
    let sequence = state.device_seq.fetch_add(1, Ordering::SeqCst) + 1;
    Json(serde_json::json!({
        "device_auth_id": format!("device-auth-{sequence}"),
        "user_code": format!("CODE-{sequence:04}"),
        "interval": "1",
    }))
}

async fn route_device_token(
    State(state): State<RouteIssuerState>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let device_auth_id = request
        .get("device_auth_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let polls = {
        let mut polls = state.device_polls.lock().expect("device poll counter");
        let entry = polls.entry(device_auth_id).or_insert(0);
        *entry += 1;
        *entry
    };
    if polls == 1 {
        // ChatGPT keeps answering 403 until the operator confirms the code.
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({})));
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "authorization_code": "auth-code-1",
            "code_verifier": "device-verifier-1",
        })),
    )
}

/// Fresh mock issuer plus the exclusive environment slot. Holding the returned
/// guard keeps `PROMPT_FERRY_CHATGPT_OAUTH_ISSUER` stable and the issuer task
/// alive for the whole test, so route tests never race on the shared variable.
async fn route_issuer() -> (MutexGuard<'static, ()>, String) {
    let guard = ROUTE_ENV_LOCK.lock().await;
    let router = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(route_usercode))
        .route("/api/accounts/deviceauth/token", post(route_device_token))
        .route("/oauth/token", post(oauth_token))
        .with_state(RouteIssuerState::default());
    let issuer = spawn_issuer(router).await;
    // SAFETY: route tests hold `ROUTE_ENV_LOCK`, so no other route test can
    // observe or overwrite the variable while this issuer is in use.
    unsafe {
        std::env::set_var("PROMPT_FERRY_CHATGPT_OAUTH_ISSUER", &issuer);
    }
    (guard, issuer)
}

struct OAuthAdminFixture {
    state: AdminState,
    store: Arc<StandaloneConfigStore>,
    path: PathBuf,
}

impl OAuthAdminFixture {
    async fn open() -> anyhow::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("prompt-ferry-oauth-{}.sqlite", Uuid::new_v4()));
        let store = Arc::new(StandaloneConfigStore::open(&path).await?);
        let user_store = db::UserStore::sqlite(store.pool().clone());
        user_store
            .bootstrap_admin("oauth-admin", "oauth-password")
            .await?;
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")?;
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
            stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: LlmReviewSettings::default(),
            mcp_catalog_cache: McpCatalogCache::new(),
            mcp_catalog_service: McpCatalogService::new(pool, McpCatalogCache::new()),
            mcp_session_store: None,
            mcp_allowed_origins: Vec::new(),
            endpoint_model_cache: EndpointModelCache::new(std::time::Duration::from_secs(60)),
        })
        .with_user_store(user_store)
        .with_config_repository(ConfigRepository::sqlite(store.clone(), test_manager()));
        state
            .replay_cache
            .write_session(
                "oauth-route-test",
                &SessionUser {
                    user_id: 1,
                    login_name: "oauth-admin".to_string(),
                    display_name: "OAuth Admin".to_string(),
                    is_admin: true,
                },
            )
            .await?;
        Ok(Self { state, store, path })
    }

    async fn create_endpoint(
        &self,
        provider: EndpointProvider,
        name: &str,
    ) -> anyhow::Result<Uuid> {
        let endpoint_id = Uuid::new_v4();
        self.state
            .config_repository
            .create_endpoint(endpoint_id, endpoint_create(provider, name), false)
            .await?;
        Ok(endpoint_id)
    }

    async fn cleanup(self) {
        let pool = self.store.pool().clone();
        drop(self.state);
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

fn test_manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([11_u8; 32])).expect("test manager")
}

fn endpoint_create(provider: EndpointProvider, name: &str) -> EndpointCreate {
    EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: name.to_string(),
        provider,
        provider_region: None,
        service_tier: Default::default(),
        base_url: "https://api.openai.com/v1".to_string(),
        native_api: NativeApi::Chat,
        native_api_source: DbNativeApiSource::Manual,
        api_key: "openai-platform-key".to_string(),
        api_keys: vec![db::EndpointApiKeyCreate {
            key_label: "primary".to_string(),
            api_key: "openai-platform-key".to_string(),
            position: 0,
            enabled: true,
            key_id: None,
        }],
        key_lb_enabled: false,
        enabled: true,
        proxy_url: None,
        active_windows: None,
    }
}

fn oauth_route(endpoint_id: Uuid, suffix: &str) -> String {
    format!("/api/v1/admin/endpoints/{endpoint_id}/oauth{suffix}")
}

fn route_request(
    method: &str,
    uri: String,
    body: Option<Value>,
    authenticated: bool,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if authenticated {
        builder = builder.header(header::COOKIE, "prompt_ferry_session=oauth-route-test");
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("oauth route request")
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body"),
    )
    .expect("JSON response")
}

#[tokio::test]
async fn admin_router_device_login_stores_token_and_reports_status() -> anyhow::Result<()> {
    let (_env_guard, issuer) = route_issuer().await;
    let fixture = OAuthAdminFixture::open().await?;
    let endpoint_id = fixture
        .create_endpoint(EndpointProvider::OpenAi, "chatgpt-device")
        .await?;
    let app = worker_admin::router(fixture.state.clone());

    let unauthorized = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            false,
        ))
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let status = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    assert_eq!(status.status(), StatusCode::OK);
    let status = response_json(status).await;
    assert_eq!(status["has_oauth_token"], false);
    assert_eq!(status["plan"], "platform_api_key");
    assert_eq!(status["expired"], false);

    let start = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/device"),
            None,
            true,
        ))
        .await?;
    assert_eq!(start.status(), StatusCode::OK);
    let start = response_json(start).await;
    let flow_id = start["flow_id"].as_str().expect("flow id").to_string();
    assert_eq!(start["verification_uri"], format!("{issuer}/codex/device"));
    assert_eq!(start["interval_seconds"], 4);
    assert!(
        start["user_code"]
            .as_str()
            .is_some_and(|code| !code.is_empty())
    );

    let poll_body = serde_json::json!({ "flow_id": flow_id });
    let pending = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/device/poll"),
            Some(poll_body.clone()),
            true,
        ))
        .await?;
    assert_eq!(pending.status(), StatusCode::OK);
    assert_eq!(response_json(pending).await["status"], "pending");

    let complete = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/device/poll"),
            Some(poll_body),
            true,
        ))
        .await?;
    assert_eq!(complete.status(), StatusCode::OK);
    let complete = response_json(complete).await;
    assert_eq!(complete["status"], "complete");
    assert_eq!(complete["endpoint"]["endpoint_id"], endpoint_id.to_string());
    assert_eq!(complete["endpoint"]["has_oauth_token"], true);
    assert_eq!(complete["endpoint"]["plan"], "chatgpt_subscription");

    let status = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    assert_eq!(status.status(), StatusCode::OK);
    let status = response_json(status).await;
    assert_eq!(status["has_oauth_token"], true);
    assert_eq!(status["plan"], "chatgpt_subscription");
    assert_eq!(status["expired"], false);

    // A valid stored token refreshes as a no-op status read.
    let refreshed = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/refresh"),
            None,
            true,
        ))
        .await?;
    assert_eq!(refreshed.status(), StatusCode::OK);
    assert_eq!(response_json(refreshed).await["has_oauth_token"], true);

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn admin_router_browser_login_completes_from_pasted_redirect() -> anyhow::Result<()> {
    let (_env_guard, issuer) = route_issuer().await;
    let fixture = OAuthAdminFixture::open().await?;
    let endpoint_id = fixture
        .create_endpoint(EndpointProvider::OpenAi, "chatgpt-browser")
        .await?;
    let app = worker_admin::router(fixture.state.clone());

    let start = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/browser"),
            None,
            true,
        ))
        .await?;
    assert_eq!(start.status(), StatusCode::OK);
    let start = response_json(start).await;
    let flow_id = start["flow_id"].as_str().expect("flow id").to_string();
    assert_eq!(start["redirect_uri"], "http://localhost:1455/auth/callback");
    let authorize_url = start["authorize_url"].as_str().expect("authorize url");
    assert!(
        authorize_url.starts_with(issuer.as_str()),
        "authorize url {authorize_url}"
    );
    let parsed = reqwest::Url::parse(authorize_url).expect("authorize url");
    let oauth_state = parsed
        .query_pairs()
        .find(|(key, _)| key.as_ref() == "state")
        .map(|(_, value)| value.into_owned())
        .expect("authorize state");

    let mismatched = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/browser/complete"),
            Some(serde_json::json!({
                "flow_id": flow_id,
                "redirect_url": "http://localhost:1455/auth/callback?code=code-1&state=wrong",
            })),
            true,
        ))
        .await?;
    assert_eq!(mismatched.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(mismatched).await["error"]["code"],
        "oauth_invalid_redirect"
    );

    let complete = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/browser/complete"),
            Some(serde_json::json!({
                "flow_id": flow_id,
                "redirect_url": format!(
                    "http://localhost:1455/auth/callback?code=code-1&state={oauth_state}"
                ),
            })),
            true,
        ))
        .await?;
    assert_eq!(complete.status(), StatusCode::OK);
    let complete = response_json(complete).await;
    assert_eq!(complete["status"], "complete");
    assert_eq!(complete["endpoint"]["endpoint_id"], endpoint_id.to_string());
    assert_eq!(complete["endpoint"]["has_oauth_token"], true);

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn admin_router_refresh_revokes_token_and_clear_and_provider_gates() -> anyhow::Result<()> {
    let _env_guard = route_issuer().await;
    let fixture = OAuthAdminFixture::open().await?;
    let endpoint_id = fixture
        .create_endpoint(EndpointProvider::OpenAi, "chatgpt-refresh")
        .await?;
    let generic_id = fixture
        .create_endpoint(EndpointProvider::Generic, "generic-endpoint")
        .await?;
    fixture
        .state
        .config_repository
        .set_endpoint_oauth_token(
            endpoint_id,
            Some(EndpointOAuthTokenSet {
                access_token: "expired-access".to_string(),
                refresh_token: "revoked-refresh".to_string(),
                expires_at: Some(Utc::now() - ChronoDuration::seconds(60)),
            }),
        )
        .await?;
    let app = worker_admin::router(fixture.state.clone());

    let status = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    let status = response_json(status).await;
    assert_eq!(status["has_oauth_token"], true);
    assert_eq!(status["expired"], true);
    assert_eq!(status["plan"], "chatgpt_subscription");

    // A revoked refresh token clears the stored credential and reports 400.
    let refresh = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/refresh"),
            None,
            true,
        ))
        .await?;
    assert_eq!(refresh.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(refresh).await["error"]["code"],
        "oauth_invalid_grant"
    );

    let status = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    let status = response_json(status).await;
    assert_eq!(status["has_oauth_token"], false);
    assert_eq!(status["plan"], "platform_api_key");
    assert_eq!(status["expired"], false);

    // Login start is OpenAI-only; clearing stays allowed on any provider.
    let unsupported = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(generic_id, "/device"),
            None,
            true,
        ))
        .await?;
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(unsupported).await["error"]["code"],
        "oauth_unsupported_provider"
    );

    let missing = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(Uuid::new_v4(), ""),
            None,
            true,
        ))
        .await?;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(response_json(missing).await["error"]["code"], "not_found");

    fixture
        .state
        .config_repository
        .set_endpoint_oauth_token(
            endpoint_id,
            Some(EndpointOAuthTokenSet {
                access_token: "fresh-access".to_string(),
                refresh_token: "fresh-refresh".to_string(),
                expires_at: Some(Utc::now() + ChronoDuration::seconds(3600)),
            }),
        )
        .await?;
    let cleared = app
        .clone()
        .oneshot(route_request(
            "DELETE",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    assert_eq!(cleared.status(), StatusCode::NO_CONTENT);
    let status = app
        .clone()
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await?;
    assert_eq!(response_json(status).await["has_oauth_token"], false);

    fixture.cleanup().await;
    Ok(())
}
