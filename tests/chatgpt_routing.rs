//! Issue #599 R2c: ChatGPT (Codex) backend routing contracts.
//!
//! These pin the private-interface mapping layer (`worker_admin::chatgpt_backend`):
//! Codex URL mapping, model normalization, request-body normalization, auth
//! headers, the display-only quota fetch, and the stored-token refresh. No
//! live ChatGPT endpoint is touched: the quota and issuer doubles are local
//! loopback servers.

use std::{
    borrow::Cow,
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{HeaderMap, Request, StatusCode, Uri, header},
    routing::{get, post},
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db::{self, ConfigRepository, EndpointCreate, EndpointOAuthTokenSet, EndpointProvider},
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    standalone_config::StandaloneConfigStore,
    worker_admin::{
        self,
        chatgpt_backend::{
            CHATGPT_BACKEND_URL_ENV, CHATGPT_CODEX_COMPACT_PATH, CHATGPT_CODEX_RESPONSES_PATH,
            ChatgptBackendError, DEFAULT_CODEX_MODEL, chatgpt_codex_path, chatgpt_codex_url,
            codex_account_id_from_access_token, fetch_chatgpt_quota, normalize_codex_model,
            normalize_codex_request_body, parse_chatgpt_quota, refresh_stored_endpoint_token,
            with_codex_headers,
        },
    },
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};
use prompt_ferry_runtime_env::relay_secrets::RelaySecretManager;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tokio::{net::TcpListener, sync::Mutex as AsyncMutex};
use tower::ServiceExt;
use uuid::Uuid;

/// Serializes the two process-global ChatGPT host overrides across the tests
/// in this file (each integration test file is its own process).
static ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

const OAUTH_ISSUER_ENV: &str = "PROMPT_FERRY_CHATGPT_OAUTH_ISSUER";

fn set_env(key: &str, value: &str) {
    // SAFETY: every env-touching test holds `ENV_LOCK`.
    unsafe {
        std::env::set_var(key, value);
    }
}

fn remove_env(key: &str) {
    // SAFETY: every env-touching test holds `ENV_LOCK`.
    unsafe {
        std::env::remove_var(key);
    }
}

async fn spawn_server(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback server");
    let address = listener.local_addr().expect("loopback address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve loopback server");
    });
    format!("http://{address}")
}

fn jwt(payload: Value) -> String {
    format!("header.{}.sig", URL_SAFE_NO_PAD.encode(payload.to_string()))
}

#[test]
fn codex_path_mapping_only_covers_responses_paths() {
    assert_eq!(
        chatgpt_codex_path("/v1/responses"),
        Some(CHATGPT_CODEX_RESPONSES_PATH)
    );
    assert_eq!(
        chatgpt_codex_path("/v1/responses/compact"),
        Some(CHATGPT_CODEX_COMPACT_PATH)
    );
    // Chat and Anthropic protocols have no Codex backend equivalent; the
    // runtime reports a clear protocol error instead of forwarding.
    assert_eq!(chatgpt_codex_path("/v1/chat/completions"), None);
    assert_eq!(chatgpt_codex_path("/v1/messages"), None);
}

#[tokio::test]
async fn codex_url_defaults_to_the_backend_root_and_honours_the_override() {
    let _guard = ENV_LOCK.lock().await;
    remove_env(CHATGPT_BACKEND_URL_ENV);
    assert_eq!(
        chatgpt_codex_url("/v1/responses").as_deref(),
        Some("https://chatgpt.com/backend-api/codex/responses")
    );
    assert_eq!(chatgpt_codex_url("/v1/chat/completions"), None);

    set_env(CHATGPT_BACKEND_URL_ENV, "http://127.0.0.1:9/backend/");
    assert_eq!(
        chatgpt_codex_url("/v1/responses/compact").as_deref(),
        Some("http://127.0.0.1:9/backend/codex/responses/compact")
    );
    remove_env(CHATGPT_BACKEND_URL_ENV);
}

#[test]
fn codex_model_normalization_covers_known_and_fallback_models() {
    for (requested, expected) in [
        ("gpt-5.2", "gpt-5.2"),
        ("gpt-5.2-codex", "gpt-5.2-codex"),
        ("gpt-5.1", "gpt-5.1"),
        ("gpt-5.1-codex", "gpt-5.1-codex"),
        ("gpt-5.1-codex-max", "gpt-5.1-codex-max"),
        ("gpt-5.1-codex-mini", "gpt-5.1-codex-mini"),
        ("gpt-5-codex", "gpt-5-codex"),
        ("gpt-5", "gpt-5"),
        ("codex-mini-latest", "codex-mini-latest"),
        // Reasoning-effort suffixes fold into the base Codex id.
        ("gpt-5.1-codex-high", "gpt-5.1-codex"),
        ("gpt-5.2-codex-xhigh", "gpt-5.2-codex"),
        ("gpt-5.1-codex-max-low", "gpt-5.1-codex-max"),
        // Provider prefix is dropped before matching.
        ("openai/gpt-5.2-codex", "gpt-5.2-codex"),
        // Non-Codex OpenAI families and unknown names fall back.
        ("gpt-4o", DEFAULT_CODEX_MODEL),
        ("o3", DEFAULT_CODEX_MODEL),
        ("gpt-4.1-mini", DEFAULT_CODEX_MODEL),
        ("legacy-model", DEFAULT_CODEX_MODEL),
        ("", DEFAULT_CODEX_MODEL),
    ] {
        assert_eq!(
            normalize_codex_model(requested),
            expected,
            "model {requested}"
        );
    }
}

#[test]
fn codex_body_normalization_maps_the_model_and_forces_store_false() {
    let body = br#"{"model":"gpt-4o","store":true,"stream":true,"input":[]}"#;
    let normalized = normalize_codex_request_body(body);
    let value: Value = serde_json::from_slice(normalized.as_ref()).unwrap();
    assert_eq!(value["model"], DEFAULT_CODEX_MODEL);
    assert_eq!(value["store"], false);
    assert_eq!(value["stream"], true);
    assert_eq!(value["input"], json!([]));

    // A model-only rewrite keeps the stateless flag when it is already false.
    let model_only = br#"{"model":"gpt-5.1-codex-high","store":false}"#;
    let rewritten = normalize_codex_request_body(model_only);
    let value: Value = serde_json::from_slice(rewritten.as_ref()).unwrap();
    assert_eq!(value["model"], "gpt-5.1-codex");
    assert_eq!(value["store"], false);
}

#[test]
fn codex_body_normalization_borrows_stable_and_unusable_bodies() {
    for stable in [
        br#"{ "model" : "gpt-5.1-codex" , "store" : false }"#.as_slice(),
        b"not-json".as_slice(),
        b"[1,2,3]".as_slice(),
    ] {
        assert!(
            matches!(normalize_codex_request_body(stable), Cow::Borrowed(_)),
            "body {:?} must stay byte-for-byte",
            String::from_utf8_lossy(stable)
        );
    }
}

#[test]
fn codex_account_id_is_read_from_the_access_token_claim() {
    let nested = jwt(json!({
        "https://api.openai.com/auth": { "chatgpt_account_id": "acct_nested" }
    }));
    assert_eq!(
        codex_account_id_from_access_token(&nested).as_deref(),
        Some("acct_nested")
    );
    let direct = jwt(json!({ "chatgpt_account_id": "acct_direct" }));
    assert_eq!(
        codex_account_id_from_access_token(&direct).as_deref(),
        Some("acct_direct")
    );
    for opaque in ["opaque-token", "header..sig", "header.@@@.sig", ""] {
        assert_eq!(codex_account_id_from_access_token(opaque), None);
    }
}

#[test]
fn codex_headers_carry_bearer_originator_and_account_binding() {
    let request = with_codex_headers(
        Client::new().get("https://chatgpt.com/backend-api/codex/responses"),
        "access-token",
        Some("acct_1"),
    )
    .build()
    .expect("codex request");
    assert_eq!(request.headers()["authorization"], "Bearer access-token");
    assert_eq!(request.headers()["originator"], "opencode");
    assert_eq!(request.headers()["chatgpt-account-id"], "acct_1");
    assert!(
        request.headers()["user-agent"]
            .to_str()
            .unwrap()
            .starts_with("prompt-ferry/")
    );

    let without = with_codex_headers(
        Client::new().get("https://example.test"),
        "access-token",
        None,
    )
    .build()
    .expect("codex request without account");
    assert!(without.headers().get("chatgpt-account-id").is_none());
}

#[test]
fn quota_parsing_reads_percent_windows_and_credits() {
    let quota = parse_chatgpt_quota(&json!({
        "plan_type": "plus",
        "rate_limit": {
            "allowed": true,
            "limit_reached": false,
            "primary_window": {
                "used_percent": 25,
                "limit_window_seconds": 18000,
                "reset_after_seconds": 14400,
                "reset_at": 1_700_000_000,
            },
            "secondary_window": {
                "used_percent": 60.5,
                "limit_window_seconds": 604800,
                "reset_after_seconds": 432000,
                "reset_at": 1_700_500_000,
            },
        },
        "credits": { "has_credits": true, "unlimited": false, "balance": "12.5" },
    }));
    assert_eq!(quota.plan_type.as_deref(), Some("plus"));
    assert_eq!(quota.limit_reached, Some(false));
    let primary = quota.primary.expect("primary window");
    assert_eq!(primary.used_percent, Some(25.0));
    assert_eq!(primary.limit_window_seconds, Some(18_000));
    assert_eq!(primary.reset_after_seconds, Some(14_400));
    assert_eq!(
        primary.reset_at,
        chrono::DateTime::from_timestamp(1_700_000_000, 0)
    );
    let secondary = quota.secondary.expect("secondary window");
    assert_eq!(secondary.used_percent, Some(60.5));
    assert_eq!(quota.has_credits, Some(true));
    assert_eq!(quota.unlimited_credits, Some(false));
    assert_eq!(quota.credits_balance.as_deref(), Some("12.5"));

    // A payload without windows degrades instead of failing.
    let empty = parse_chatgpt_quota(&json!({}));
    assert!(empty.primary.is_none());
    assert!(empty.secondary.is_none());
    assert!(empty.plan_type.is_none());
}

#[derive(Default)]
struct UsageServerState {
    /// Reply 404 on `/wham/usage` so the compatibility fallback is exercised.
    wham_not_found: bool,
    /// Reply 401 on every usage path (auth failure).
    unauthorized: bool,
    paths: StdMutex<Vec<String>>,
    authorizations: StdMutex<Vec<String>>,
    account_ids: StdMutex<Vec<String>>,
}

async fn usage_handler(
    State(state): State<Arc<UsageServerState>>,
    headers: HeaderMap,
    uri: Uri,
) -> (StatusCode, axum::Json<Value>) {
    state
        .paths
        .lock()
        .expect("usage paths")
        .push(uri.path().to_string());
    state
        .authorizations
        .lock()
        .expect("usage auth headers")
        .push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
        );
    state
        .account_ids
        .lock()
        .expect("usage account headers")
        .push(
            headers
                .get("chatgpt-account-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
        );
    if state.unauthorized {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({})));
    }
    if state.wham_not_found && uri.path() == "/wham/usage" {
        return (StatusCode::NOT_FOUND, axum::Json(json!({})));
    }
    (
        StatusCode::OK,
        axum::Json(json!({
            "plan_type": "plus",
            "rate_limit": {
                "limit_reached": false,
                "primary_window": { "used_percent": 10, "reset_after_seconds": 1440 },
            },
        })),
    )
}

async fn spawn_usage_server(state: Arc<UsageServerState>) -> String {
    spawn_server(
        Router::new()
            .route("/wham/usage", get(usage_handler))
            .route("/codex/usage", get(usage_handler))
            .with_state(state),
    )
    .await
}

#[tokio::test]
async fn quota_fetch_prefers_wham_and_falls_back_to_codex_usage() {
    let _guard = ENV_LOCK.lock().await;
    let state = Arc::new(UsageServerState {
        wham_not_found: true,
        ..UsageServerState::default()
    });
    let base = spawn_usage_server(state.clone()).await;
    set_env(CHATGPT_BACKEND_URL_ENV, &base);

    let quota = fetch_chatgpt_quota(&Client::new(), "access-token", Some("acct_1"))
        .await
        .expect("quota fetch");
    remove_env(CHATGPT_BACKEND_URL_ENV);

    assert_eq!(quota.plan_type.as_deref(), Some("plus"));
    assert_eq!(
        quota.primary.expect("primary window").used_percent,
        Some(10.0)
    );
    assert_eq!(
        state.paths.lock().unwrap().as_slice(),
        ["/wham/usage", "/codex/usage"]
    );
    assert_eq!(
        state.authorizations.lock().unwrap().as_slice(),
        ["Bearer access-token", "Bearer access-token"]
    );
    assert_eq!(
        state.account_ids.lock().unwrap().as_slice(),
        ["acct_1", "acct_1"]
    );
}

#[tokio::test]
async fn quota_fetch_reports_auth_failures_without_falling_back() {
    let _guard = ENV_LOCK.lock().await;
    let state = Arc::new(UsageServerState {
        unauthorized: true,
        ..UsageServerState::default()
    });
    let base = spawn_usage_server(state.clone()).await;
    set_env(CHATGPT_BACKEND_URL_ENV, &base);

    let error = fetch_chatgpt_quota(&Client::new(), "access-token", None)
        .await
        .expect_err("auth failure");
    remove_env(CHATGPT_BACKEND_URL_ENV);

    match error {
        ChatgptBackendError::Upstream { status, .. } => assert_eq!(status, Some(401)),
        other => panic!("expected an upstream error, got {other}"),
    }
    assert_eq!(state.paths.lock().unwrap().as_slice(), ["/wham/usage"]);
}

fn test_manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([21_u8; 32])).expect("test manager")
}

fn endpoint_create() -> EndpointCreate {
    endpoint_create_for(EndpointProvider::OpenAi)
}

fn endpoint_create_for(provider: EndpointProvider) -> EndpointCreate {
    EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: format!("chatgpt-{}", Uuid::new_v4()),
        provider,
        provider_region: None,
        service_tier: Default::default(),
        base_url: "https://api.openai.com/v1".to_string(),
        native_api: NativeApi::Responses,
        native_api_source: NativeApiSource::Manual,
        api_key: "platform-key".to_string(),
        api_keys: Vec::new(),
        key_lb_enabled: false,
        enabled: true,
        proxy_url: None,
        active_windows: None,
    }
}

struct SqliteFixture {
    repository: ConfigRepository,
    store: Arc<StandaloneConfigStore>,
    path: PathBuf,
}

impl SqliteFixture {
    async fn open() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-chatgpt-routing-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let repository = ConfigRepository::sqlite(store.clone(), test_manager());
        Self {
            repository,
            store,
            path,
        }
    }

    async fn create_endpoint(&self) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        self.repository
            .create_endpoint(endpoint_id, endpoint_create(), false)
            .await
            .expect("create endpoint");
        endpoint_id
    }

    async fn store_token(&self, endpoint_id: Uuid, access: &str, refresh: &str) {
        self.repository
            .set_endpoint_oauth_token(
                endpoint_id,
                Some(EndpointOAuthTokenSet {
                    access_token: access.to_string(),
                    refresh_token: refresh.to_string(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::seconds(60)),
                }),
            )
            .await
            .expect("store token");
    }

    async fn stored_presence(&self, endpoint_id: Uuid) -> (bool, db::EndpointPlan) {
        let endpoint = self
            .repository
            .get_endpoint(endpoint_id)
            .await
            .expect("read endpoint")
            .expect("endpoint present");
        (endpoint.has_oauth_token, endpoint.plan)
    }

    async fn token_ids(&self) -> Vec<Uuid> {
        self.repository
            .list_endpoint_oauth_token_ids()
            .await
            .expect("read token ids")
    }

    async fn cleanup(self) {
        let pool = self.store.pool().clone();
        drop(self.repository);
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

#[derive(Default)]
struct IssuerState {
    revoked: bool,
    /// Reply without a rotated refresh token on refresh grants.
    omit_refresh_rotation: bool,
    grants: StdMutex<Vec<String>>,
}

async fn issuer_token(
    State(state): State<Arc<IssuerState>>,
    body: Bytes,
) -> (StatusCode, axum::Json<Value>) {
    let body = String::from_utf8_lossy(&body);
    let params = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect::<HashMap<_, _>>();
    let grant_type = params
        .get("grant_type")
        .copied()
        .unwrap_or_default()
        .to_string();
    state
        .grants
        .lock()
        .expect("issuer grants")
        .push(grant_type.clone());
    if state.revoked {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({
                "error": "invalid_grant",
                "error_description": "refresh token revoked",
            })),
        );
    }
    if state.omit_refresh_rotation && grant_type == "refresh_token" {
        return (
            StatusCode::OK,
            axum::Json(json!({ "access_token": "access-2", "expires_in": 60 })),
        );
    }
    let (access_token, refresh_token) = if grant_type == "refresh_token" {
        ("access-rotated", Some("refresh-rotated"))
    } else {
        ("access-login", Some("refresh-login"))
    };
    (
        StatusCode::OK,
        axum::Json(json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_in": 1800,
        })),
    )
}

async fn spawn_issuer(state: Arc<IssuerState>) -> String {
    spawn_server(
        Router::new()
            .route("/oauth/token", post(issuer_token))
            .with_state(state),
    )
    .await
}

async fn issuer_usercode() -> (StatusCode, axum::Json<Value>) {
    (
        StatusCode::OK,
        axum::Json(json!({
            "device_auth_id": "device-auth-1",
            "user_code": "ABCD-1234",
            "interval": "1",
        })),
    )
}

async fn issuer_device_token() -> (StatusCode, axum::Json<Value>) {
    (
        StatusCode::OK,
        axum::Json(json!({
            "authorization_code": "auth-code-1",
            "code_verifier": "device-verifier-1",
        })),
    )
}

/// Login-capable issuer double: device start/poll plus the token endpoint.
async fn spawn_login_issuer(state: Arc<IssuerState>) -> String {
    spawn_server(
        Router::new()
            .route("/api/accounts/deviceauth/usercode", post(issuer_usercode))
            .route("/api/accounts/deviceauth/token", post(issuer_device_token))
            .route("/oauth/token", post(issuer_token))
            .with_state(state),
    )
    .await
}

#[tokio::test]
async fn refresh_rotates_the_stored_token_on_success() {
    let _guard = ENV_LOCK.lock().await;
    let fixture = SqliteFixture::open().await;
    let endpoint_id = fixture.create_endpoint().await;
    fixture
        .store_token(endpoint_id, "access-old", "refresh-old")
        .await;
    let issuer_state = Arc::new(IssuerState::default());
    let issuer = spawn_issuer(issuer_state.clone()).await;
    set_env(OAUTH_ISSUER_ENV, &issuer);

    let tokens = refresh_stored_endpoint_token(&fixture.repository, &Client::new(), endpoint_id)
        .await
        .expect("refresh");
    remove_env(OAUTH_ISSUER_ENV);

    assert_eq!(tokens.access_token, "access-rotated");
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-rotated"));
    assert_eq!(
        issuer_state.grants.lock().unwrap().as_slice(),
        ["refresh_token"]
    );
    let (has_token, plan) = fixture.stored_presence(endpoint_id).await;
    assert!(has_token, "rotated token persisted");
    assert_eq!(plan, db::EndpointPlan::ChatgptSubscription);
    assert!(
        fixture.token_ids().await.contains(&endpoint_id),
        "rotated token persisted"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn refresh_revoked_grant_clears_the_stored_token() {
    let _guard = ENV_LOCK.lock().await;
    let fixture = SqliteFixture::open().await;
    let endpoint_id = fixture.create_endpoint().await;
    fixture
        .store_token(endpoint_id, "access-old", "refresh-old")
        .await;
    let issuer = spawn_issuer(Arc::new(IssuerState {
        revoked: true,
        ..IssuerState::default()
    }))
    .await;
    set_env(OAUTH_ISSUER_ENV, &issuer);

    let Err(error) =
        refresh_stored_endpoint_token(&fixture.repository, &Client::new(), endpoint_id).await
    else {
        panic!("revoked grant must fail");
    };
    remove_env(OAUTH_ISSUER_ENV);

    match error {
        ChatgptBackendError::InvalidGrant(message) => {
            assert!(message.contains("revoked"), "{message}");
        }
        other => panic!("expected invalid_grant, got {other}"),
    }
    let (has_token, _) = fixture.stored_presence(endpoint_id).await;
    assert!(!has_token, "a revoked grant must clear the stored token");
    assert!(
        fixture.token_ids().await.is_empty(),
        "a revoked grant must clear the stored token"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn refresh_keeps_the_stored_refresh_token_when_rotation_is_omitted() {
    let _guard = ENV_LOCK.lock().await;
    let fixture = SqliteFixture::open().await;
    let endpoint_id = fixture.create_endpoint().await;
    fixture
        .store_token(endpoint_id, "access-old", "refresh-old")
        .await;
    let issuer = spawn_issuer(Arc::new(IssuerState {
        omit_refresh_rotation: true,
        ..IssuerState::default()
    }))
    .await;
    set_env(OAUTH_ISSUER_ENV, &issuer);

    let tokens = refresh_stored_endpoint_token(&fixture.repository, &Client::new(), endpoint_id)
        .await
        .expect("refresh");
    remove_env(OAUTH_ISSUER_ENV);

    assert_eq!(tokens.access_token, "access-2");
    assert!(
        tokens.refresh_token.is_none(),
        "omitted rotation returns no refresh token"
    );
    let (has_token, plan) = fixture.stored_presence(endpoint_id).await;
    assert!(has_token, "stored refresh token is kept");
    assert_eq!(plan, db::EndpointPlan::ChatgptSubscription);
    assert!(
        fixture.token_ids().await.contains(&endpoint_id),
        "stored refresh token is kept"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn refresh_without_a_stored_token_is_not_configured() {
    let fixture = SqliteFixture::open().await;
    let endpoint_id = fixture.create_endpoint().await;
    let Err(error) =
        refresh_stored_endpoint_token(&fixture.repository, &Client::new(), endpoint_id).await
    else {
        panic!("missing token must fail");
    };
    assert!(matches!(error, ChatgptBackendError::NotConfigured(_)));
    fixture.cleanup().await;
}

// ---- Admin route surface: quota display + completion-time provider gate ----

struct AdminFixture {
    state: AdminState,
    store: Arc<StandaloneConfigStore>,
    path: PathBuf,
}

impl AdminFixture {
    async fn open() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-chatgpt-admin-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let user_store = db::UserStore::sqlite(store.pool().clone());
        user_store
            .bootstrap_admin("chatgpt-admin", "chatgpt-password")
            .await
            .expect("bootstrap admin");
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")
            .expect("lazy pool");
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
            endpoint_model_cache: EndpointModelCache::new(Duration::from_secs(60)),
        })
        .with_user_store(user_store)
        .with_config_repository(ConfigRepository::sqlite(store.clone(), test_manager()));
        state
            .replay_cache
            .write_session(
                "chatgpt-route-test",
                &SessionUser {
                    user_id: 1,
                    login_name: "chatgpt-admin".to_string(),
                    display_name: "ChatGPT Admin".to_string(),
                    is_admin: true,
                },
            )
            .await
            .expect("session");
        Self { state, store, path }
    }

    async fn create_endpoint(&self, provider: EndpointProvider) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        self.state
            .config_repository
            .create_endpoint(endpoint_id, endpoint_create_for(provider), false)
            .await
            .expect("create endpoint");
        endpoint_id
    }

    async fn set_provider(&self, endpoint_id: Uuid, provider: EndpointProvider) {
        self.state
            .config_repository
            .update_endpoint(endpoint_id, endpoint_create_for(provider))
            .await
            .expect("update endpoint");
    }

    async fn store_token(&self, endpoint_id: Uuid) {
        self.state
            .config_repository
            .set_endpoint_oauth_token(
                endpoint_id,
                Some(EndpointOAuthTokenSet {
                    access_token: "access-token".to_string(),
                    refresh_token: "refresh-token".to_string(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::seconds(3600)),
                }),
            )
            .await
            .expect("store token");
    }

    async fn cleanup(self) {
        let pool = self.store.pool().clone();
        drop(self.state);
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

fn route_request(
    method: &str,
    uri: String,
    body: Option<Value>,
    authenticated: bool,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if authenticated {
        builder = builder.header(header::COOKIE, "prompt_ferry_session=chatgpt-route-test");
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("route request")
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body"),
    )
    .expect("JSON response")
}

fn oauth_route(endpoint_id: Uuid, suffix: &str) -> String {
    format!("/api/v1/admin/endpoints/{endpoint_id}/oauth{suffix}")
}

#[tokio::test]
async fn admin_token_plan_usage_returns_chatgpt_windows_and_requires_a_login() {
    let _guard = ENV_LOCK.lock().await;
    let fixture = AdminFixture::open().await;
    let endpoint_id = fixture.create_endpoint(EndpointProvider::OpenAi).await;
    let usage_ref = Arc::new(UsageServerState::default());
    let backend = spawn_usage_server(usage_ref.clone()).await;
    set_env(CHATGPT_BACKEND_URL_ENV, &backend);
    let app = worker_admin::router(fixture.state.clone());

    // Without a stored token the route reports the missing login instead of a
    // misleading provider error.
    let missing = app
        .clone()
        .oneshot(route_request(
            "GET",
            format!("/api/v1/admin/endpoints/{endpoint_id}/token-plan-usage"),
            None,
            true,
        ))
        .await
        .expect("usage request");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(missing).await["error"]["code"],
        "oauth_login_required"
    );

    fixture.store_token(endpoint_id).await;
    let response = app
        .oneshot(route_request(
            "GET",
            format!("/api/v1/admin/endpoints/{endpoint_id}/token-plan-usage"),
            None,
            true,
        ))
        .await
        .expect("usage request");
    remove_env(CHATGPT_BACKEND_URL_ENV);
    assert_eq!(response.status(), StatusCode::OK);
    let usage = response_json(response).await;
    assert_eq!(usage["provider"], "openai");
    assert_eq!(usage["keys"][0]["ok"], true);
    let window = &usage["keys"][0]["model_remains"][0]["interval"];
    assert_eq!(window["remaining_percent"], json!(90.0));
    assert_eq!(window["remains_time_ms"], json!(1_440_000));
    assert_eq!(
        usage_ref.paths.lock().unwrap().as_slice(),
        ["/wham/usage"],
        "a token-less request must not reach the backend"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn oauth_completion_rechecks_the_provider_gate() {
    let _guard = ENV_LOCK.lock().await;
    let fixture = AdminFixture::open().await;
    let endpoint_id = fixture.create_endpoint(EndpointProvider::OpenAi).await;
    let issuer_state = Arc::new(IssuerState::default());
    let issuer = spawn_login_issuer(issuer_state.clone()).await;
    set_env(OAUTH_ISSUER_ENV, &issuer);
    let app = worker_admin::router(fixture.state.clone());

    let start = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/device"),
            None,
            true,
        ))
        .await
        .expect("device start");
    assert_eq!(start.status(), StatusCode::OK);
    let flow_id = response_json(start).await["flow_id"]
        .as_str()
        .expect("flow id")
        .to_string();

    // The endpoint moves off OpenAI inside the flow TTL.
    fixture
        .set_provider(endpoint_id, EndpointProvider::Generic)
        .await;

    let poll = app
        .clone()
        .oneshot(route_request(
            "POST",
            oauth_route(endpoint_id, "/device/poll"),
            Some(json!({ "flow_id": flow_id })),
            true,
        ))
        .await
        .expect("device poll");
    remove_env(OAUTH_ISSUER_ENV);
    assert_eq!(poll.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(poll).await["error"]["code"],
        "oauth_unsupported_provider"
    );
    assert!(
        issuer_state.grants.lock().unwrap().is_empty(),
        "no token exchange may run after the provider moved off OpenAI"
    );

    let status = app
        .oneshot(route_request(
            "GET",
            oauth_route(endpoint_id, ""),
            None,
            true,
        ))
        .await
        .expect("status");
    assert_eq!(response_json(status).await["has_oauth_token"], false);

    fixture.cleanup().await;
}
