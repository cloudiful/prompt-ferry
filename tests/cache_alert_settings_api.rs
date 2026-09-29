#[path = "support/db_harness.rs"]
mod db_harness;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db;
use sqlx::PgPool;
use tower::ServiceExt;

/// The cache-alert response reports real stored-secret presence without ever
/// echoing the secret: false before any write, false when only an unknown
/// `has_dingtalk_secret` request field is sent, true once a non-blank secret is
/// stored, and still true after a blank (keep) write. An out-of-range threshold
/// is rejected with 400 and leaves storage alone.
#[tokio::test]
async fn cache_alert_settings_api_tracks_secret_presence_without_echoing_it() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let app = settings_router(&schema.pool).await?;

    // Never configured: presence is false and the secret stays blank.
    let body = send(&app, get_request()).await?;
    assert_eq!(body["has_dingtalk_secret"], false);
    assert_eq!(body["dingtalk_secret"], "");

    let invalid = serde_json::json!({
        "enabled": true,
        "threshold": 5.0,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
    });
    let response = app.clone().oneshot(settings_request(invalid)).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        db::get_cache_alert_settings(&schema.pool)
            .await?
            .dingtalk_secret
            .is_empty()
    );

    // A `has_dingtalk_secret` smuggled into the request is ignored and never
    // persisted, so presence keeps tracking the (still blank) stored secret.
    let ignored_flag = serde_json::json!({
        "enabled": true,
        "window_minutes": 15,
        "min_turns": 6,
        "threshold": 0.35,
        "cooldown_minutes": 120,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
        "has_dingtalk_secret": true,
    });
    let body = send(&app, settings_request(ignored_flag)).await?;
    assert_eq!(body["has_dingtalk_secret"], false);
    assert_eq!(body["dingtalk_secret"], "");
    assert!(
        db::get_cache_alert_settings(&schema.pool)
            .await?
            .dingtalk_secret
            .is_empty()
    );

    // A non-blank secret is stored; presence flips to true and the echo is redacted.
    let valid = serde_json::json!({
        "enabled": true,
        "window_minutes": 15,
        "min_turns": 6,
        "threshold": 0.35,
        "cooldown_minutes": 120,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
        "dingtalk_secret": "SEC-test-secret",
    });
    let body = send(&app, settings_request(valid)).await?;
    assert_eq!(body["has_dingtalk_secret"], true);
    assert_eq!(body["dingtalk_secret"], "");
    assert_eq!(body["window_minutes"], 15);

    // The GET echo matches the stored policy except for the write-only secret.
    let body = send(&app, get_request()).await?;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["min_turns"], 6);
    assert_eq!(body["cooldown_minutes"], 120);
    assert_eq!(body["dingtalk_secret"], "");
    assert_eq!(body["has_dingtalk_secret"], true);
    let stored = db::get_cache_alert_settings(&schema.pool).await?;
    assert_eq!(stored.dingtalk_secret, "SEC-test-secret");
    assert!((stored.threshold - 0.35).abs() < 1e-12);

    // A blank secret on the next write keeps the stored one, so presence stays
    // true even though the request asked for `false`.
    let keep_secret = serde_json::json!({
        "enabled": true,
        "window_minutes": 15,
        "min_turns": 6,
        "threshold": 0.35,
        "cooldown_minutes": 120,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
        "dingtalk_secret": "",
        "has_dingtalk_secret": false,
    });
    let body = send(&app, settings_request(keep_secret)).await?;
    assert_eq!(body["has_dingtalk_secret"], true);
    assert_eq!(body["dingtalk_secret"], "");
    let stored = db::get_cache_alert_settings(&schema.pool).await?;
    assert_eq!(stored.dingtalk_secret, "SEC-test-secret");

    schema.cleanup().await?;
    Ok(())
}

async fn send(app: &Router, request: Request<Body>) -> anyhow::Result<serde_json::Value> {
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX).await?,
    )?)
}

fn get_request() -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/api/v1/settings/cache-alert")
        .header(header::COOKIE, "prompt_ferry_session=test-session")
        .body(Body::empty())
        .expect("settings request")
}

fn settings_request(body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri("/api/v1/settings/cache-alert")
        .header(header::COOKIE, "prompt_ferry_session=test-session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("settings request")
}

/// Real-PostgreSQL admin state with a pre-authenticated admin session, so the
/// settings handlers exercise their storage path instead of a lazy pool.
async fn settings_router(pool: &PgPool) -> anyhow::Result<Router> {
    use prompt_ferry::{
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
    use std::time::Duration;

    let admin = db::create_user(
        pool,
        db::UserCreate {
            login_name: "cache-alert-admin".to_string(),
            password_hash: prompt_ferry::keys::hash_password("password-123")?,
            display_name: "cache-alert-admin".to_string(),
            is_admin: true,
        },
    )
    .await?;
    let replay_cache = ReplayCache::for_tests();
    replay_cache
        .write_session(
            "test-session",
            &SessionUser {
                user_id: admin.user_id,
                login_name: admin.login_name.clone(),
                display_name: admin.display_name.clone(),
                is_admin: true,
            },
        )
        .await?;
    let state = AdminState::new(AdminStateInit {
        pool: pool.clone(),
        lease_pool: pool.clone(),
        replay_cache,
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
        mcp_catalog_service: McpCatalogService::new(pool.clone(), McpCatalogCache::new()),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        endpoint_model_cache: EndpointModelCache::new(Duration::from_secs(60)),
    });
    Ok(worker_admin::router(state))
}
