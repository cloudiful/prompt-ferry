#[path = "support/db_harness.rs"]
mod db_harness;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db;
use sqlx::PgPool;
use tower::ServiceExt;

/// PUT rejects an out-of-range threshold with 400 and leaves storage alone;
/// a valid write persists the normalized policy and the GET echo hides the
/// write-only DingTalk secret.
#[tokio::test]
async fn cache_alert_settings_api_rejects_invalid_threshold_and_hides_secret() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let app = settings_router(&schema.pool).await?;

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

    let valid = serde_json::json!({
        "enabled": true,
        "window_minutes": 15,
        "min_turns": 6,
        "threshold": 0.35,
        "cooldown_minutes": 120,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
        "dingtalk_secret": "SEC-test-secret",
    });
    let response = app.clone().oneshot(settings_request(valid)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["dingtalk_secret"], "");
    assert_eq!(body["window_minutes"], 15);

    // The GET echo matches the stored policy except for the write-only secret.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/settings/cache-alert")
                .header(header::COOKIE, "prompt_ferry_session=test-session")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["min_turns"], 6);
    assert_eq!(body["cooldown_minutes"], 120);
    assert_eq!(body["dingtalk_secret"], "");
    let stored = db::get_cache_alert_settings(&schema.pool).await?;
    assert_eq!(stored.dingtalk_secret, "SEC-test-secret");
    assert!((stored.threshold - 0.35).abs() < 1e-12);

    // A blank secret on the next write keeps the stored one.
    let keep_secret = serde_json::json!({
        "enabled": true,
        "window_minutes": 15,
        "min_turns": 6,
        "threshold": 0.35,
        "cooldown_minutes": 120,
        "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test",
        "dingtalk_secret": "",
    });
    let response = app.oneshot(settings_request(keep_secret)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = db::get_cache_alert_settings(&schema.pool).await?;
    assert_eq!(stored.dingtalk_secret, "SEC-test-secret");

    schema.cleanup().await?;
    Ok(())
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
async fn settings_router(pool: &PgPool) -> anyhow::Result<axum::Router> {
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
