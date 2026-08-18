use std::{env, str::FromStr, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db,
    keys::hash_password,
    llm_review::{ApprovalResolution, LlmReviewSettings},
    mcp::{McpCatalogCache, McpCatalogService},
    protocol::BridgeMessage,
    replay_cache::ReplayCache,
    worker_admin,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};
use serde_json::Value;
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::mpsc;
use tower::ServiceExt;
use uuid::Uuid;

const TEST_DATABASE_URL_ENV: &str = "PROMPT_FERRY_TEST_DATABASE_URL";

struct TestSchema {
    pool: PgPool,
    admin_pool: PgPool,
    schema: String,
}

impl TestSchema {
    async fn new() -> anyhow::Result<Self> {
        let database_url = env::var(TEST_DATABASE_URL_ENV)?;
        let schema = format!("pfy_test_{}", Uuid::new_v4().simple());

        let base_options = PgConnectOptions::from_str(&database_url)?;
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(base_options.clone())
            .await?;
        admin_pool
            .execute(sqlx::AssertSqlSafe(format!(
                r#"CREATE SCHEMA "{}""#,
                schema
            )))
            .await?;

        let schema_options = base_options.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(schema_options)
            .await?;
        db::migrate(&pool).await?;

        Ok(Self {
            pool,
            admin_pool,
            schema,
        })
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        self.admin_pool
            .execute(sqlx::AssertSqlSafe(format!(
                r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#,
                self.schema
            )))
            .await?;
        self.pool.close().await;
        self.admin_pool.close().await;
        Ok(())
    }
}

fn test_database_configured() -> bool {
    env::var(TEST_DATABASE_URL_ENV).is_ok()
}

async fn create_user(pool: &PgPool, login_name: &str, is_admin: bool) -> anyhow::Result<db::User> {
    db::create_user(
        pool,
        db::UserCreate {
            login_name: login_name.to_string(),
            password_hash: hash_password("password-123")?,
            display_name: login_name.to_string(),
            is_admin,
        },
    )
    .await
}

async fn admin_state(pool: PgPool, admin: &db::User) -> AdminState {
    let replay_cache = ReplayCache::for_tests();
    let state = AdminState::new(AdminStateInit {
        pool: pool.clone(),
        lease_pool: pool.clone(),
        replay_cache: replay_cache.clone(),
        configured_relays: vec!["ws://relay:8788/ws/worker".to_string()],
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
        mcp_quota_valkey: prompt_ferry::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: prompt_ferry::endpoint_models::EndpointModelCache::new(
            Duration::from_secs(300),
        ),
    });
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
        .await
        .unwrap();
    state
}

async fn create_pending_approval(
    pool: &PgPool,
    user_id: i64,
) -> anyhow::Result<db::ApprovalRequest> {
    db::create_flagged_approval_request(
        pool,
        db::FlaggedApprovalRequestInput {
            request_id: Uuid::new_v4(),
            user_id: Some(user_id),
            client_key_label: Some("ops-key".to_string()),
            path: "/v1/chat/completions".to_string(),
            model: Some("gpt-test".to_string()),
            review_reason: "needs human review".to_string(),
            review_categories: vec!["policy".to_string()],
            request_preview: "user: hello".to_string(),
            request_payload_json: serde_json::json!({ "model": "gpt-test", "messages": [{ "role": "user", "content": "hello" }] }),
            request_deadline_unix_ms: 400_000,
            wait_deadline_unix_ms: 300_000,
        },
    )
    .await
}

async fn insert_request_record(pool: &PgPool, user_id: i64, model: &str) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(Some(user_id), None, None, None)
            .with_model(Some(model.to_string()))
            .with_timing(Some(200), Some(true), Some(10), Some(1))
            .with_usage(Some(1), Some(2), Some(3), Some(0), None, None),
    )
    .await
}

async fn insert_mcp_request_record(
    pool: &PgPool,
    user_id: i64,
    server_name: &str,
) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::mcp_request(Uuid::new_v4(), "/mcp")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(Some(user_id), None, None, None)
            .with_mcp_context(
                None,
                Some(server_name.to_string()),
                Some("tools/call".to_string()),
                Some("demo__lookup".to_string()),
            )
            .with_timing(Some(200), Some(true), Some(8), None),
    )
    .await
}

fn auth_request(method: &str, path: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, "prompt_ferry_session=test-session")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn approve_endpoint_wakes_waiter_and_clears_payload() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let approval = create_pending_approval(&schema.pool, admin.user_id).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .approval_waiters
        .lock()
        .await
        .insert(approval.approval_id, tx);

    let app = worker_admin::router(state.clone());
    let response = app
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/approvals/{}/approve", approval.approval_id),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), rx)
            .await
            .unwrap()
            .unwrap(),
        ApprovalResolution::Approved
    );

    let stored = db::get_approval_request(&schema.pool, approval.approval_id)
        .await?
        .unwrap();
    assert_eq!(stored.approval_status, "approved");
    assert!(stored.request_payload_json.is_none());
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn login_succeeds_with_local_session_fallback_when_session_backend_is_disabled()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-login", true).await?;
    let state = AdminState::new(AdminStateInit {
        pool: schema.pool.clone(),
        lease_pool: schema.pool.clone(),
        replay_cache: ReplayCache::default(),
        configured_relays: vec!["ws://relay:8788/ws/worker".to_string()],
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
        mcp_catalog_service: McpCatalogService::new(schema.pool.clone(), McpCatalogCache::new()),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        mcp_quota_valkey: prompt_ferry::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: prompt_ferry::endpoint_models::EndpointModelCache::new(
            Duration::from_secs(300),
        ),
    });
    let app = worker_admin::router(state);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "login_name": admin.login_name,
                "password": "password-123"
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header");
    assert!(set_cookie.to_str()?.contains("prompt_ferry_session="));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn auth_me_reads_session_from_valkey_backend() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-me", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let app = worker_admin::router(state);
    let response = app
        .oneshot(auth_request("GET", "/api/v1/auth/me".to_string()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn endpoint_key_override_and_request_snapshot_are_preserved() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping endpoint key integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-endpoint-key", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "endpoint-key-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://endpoint-key.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![
                db::EndpointApiKeyCreate {
                    key_label: "primary".to_string(),
                    api_key: "primary-secret".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: None,
                },
                db::EndpointApiKeyCreate {
                    key_label: "secondary".to_string(),
                    api_key: "secondary-secret".to_string(),
                    position: 1,
                    enabled: true,
                    key_id: None,
                },
                db::EndpointApiKeyCreate {
                    key_label: "disabled".to_string(),
                    api_key: "disabled-secret".to_string(),
                    position: 2,
                    enabled: false,
                    key_id: None,
                },
            ],
            key_lb_enabled: true,
            enabled: true,
        },
    )
    .await?;
    let primary = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "primary")
        .expect("primary endpoint key")
        .clone();
    let secondary = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "secondary")
        .expect("secondary endpoint key")
        .clone();
    let disabled = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "disabled")
        .expect("disabled endpoint key")
        .clone();
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(Some(admin.user_id), None, None, None)
            .with_route(Some(endpoint.endpoint_id), None)
            .with_endpoint_key(Some(primary.key_id), Some(primary.key_label.clone()))
            .with_model(Some("gpt-endpoint-key".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;

    let mut request = auth_request(
        "PUT",
        format!("/api/v1/admin/conversations/{conversation_id}/endpoint-override"),
    );
    *request.body_mut() = Body::from(
        serde_json::json!({
            "endpoint_id": endpoint.endpoint_id,
            "endpoint_key_id": secondary.key_id,
        })
        .to_string(),
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("content type"),
    );
    let response = worker_admin::router(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["endpoint_key_id"], secondary.key_id.to_string());
    assert_eq!(body["endpoint_key_label"], "secondary");
    assert!(body.get("api_key").is_none());

    let mut invalid_request = auth_request(
        "PUT",
        format!("/api/v1/admin/conversations/{conversation_id}/endpoint-override"),
    );
    *invalid_request.body_mut() = Body::from(
        serde_json::json!({
            "endpoint_id": endpoint.endpoint_id,
            "endpoint_key_id": disabled.key_id,
        })
        .to_string(),
    );
    invalid_request.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("content type"),
    );
    let response = worker_admin::router(state.clone())
        .oneshot(invalid_request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["error"]["code"], "invalid_endpoint_key");

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "GET",
            format!("/api/v1/admin/request-records/{record_id}/session-route-options"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(
        body["override_endpoint_key_id"],
        secondary.key_id.to_string()
    );
    assert_eq!(body["options"][0]["keys"][0]["key_label"], "primary");
    assert!(body.to_string().contains("primary"));
    assert!(!body.to_string().contains("primary-secret"));
    assert!(!body.to_string().contains("secondary-secret"));

    db::update_endpoint(
        &schema.pool,
        endpoint.endpoint_id,
        db::EndpointCreate {
            scope: endpoint.scope.clone(),
            owner_user_id: endpoint.owner_user_id,
            name: endpoint.name.clone(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: endpoint.base_url.clone(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: endpoint.daily_max_requests,
            monthly_max_requests: endpoint.monthly_max_requests,
            api_key: endpoint.api_key.clone(),
            api_keys: vec![db::EndpointApiKeyCreate {
                key_label: secondary.key_label.clone(),
                api_key: secondary.api_key.clone(),
                position: secondary.position,
                enabled: true,
                key_id: Some(secondary.key_id),
            }],
            key_lb_enabled: endpoint.key_lb_enabled,
            enabled: endpoint.enabled,
        },
    )
    .await?;
    let override_after_key_delete =
        db::get_conversation_endpoint_override(&schema.pool, conversation_id)
            .await?
            .expect("endpoint override after key deletion");
    assert_eq!(override_after_key_delete.endpoint_id, endpoint.endpoint_id);
    assert_eq!(
        override_after_key_delete.endpoint_key_id,
        Some(secondary.key_id),
        "override key must survive the key update because the key_id is preserved"
    );
    let detail = db::get_visible_usage_event_detail(&schema.pool, record_id, None)
        .await?
        .expect("request detail after key deletion");
    assert_eq!(detail.endpoint_key_id, Some(primary.key_id));
    assert_eq!(detail.endpoint_key_label.as_deref(), Some("primary"));

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "DELETE",
            format!("/api/v1/admin/conversations/{conversation_id}/endpoint-override"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        db::get_conversation_endpoint_override(&schema.pool, conversation_id)
            .await?
            .is_none()
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn endpoint_key_update_preserves_key_identity() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping endpoint key integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "key-identity-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://key-identity.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![
                db::EndpointApiKeyCreate {
                    key_label: "primary".to_string(),
                    api_key: "primary-secret".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: None,
                },
                db::EndpointApiKeyCreate {
                    key_label: "secondary".to_string(),
                    api_key: "secondary-secret".to_string(),
                    position: 1,
                    enabled: true,
                    key_id: None,
                },
            ],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let primary_id = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "primary")
        .expect("primary endpoint key")
        .key_id;
    let secondary_id = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "secondary")
        .expect("secondary endpoint key")
        .key_id;

    let legacy_updated = db::update_endpoint(
        &schema.pool,
        endpoint.endpoint_id,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "key-identity-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://key-identity.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![
                db::EndpointApiKeyCreate {
                    key_label: "primary".to_string(),
                    api_key: "rotated-primary-secret".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: None,
                },
                db::EndpointApiKeyCreate {
                    key_label: "secondary".to_string(),
                    api_key: "".to_string(),
                    position: 1,
                    enabled: true,
                    key_id: None,
                },
            ],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?
    .expect("endpoint updated");
    assert_eq!(
        legacy_updated
            .api_keys
            .iter()
            .find(|key| key.key_label == "primary")
            .map(|key| key.key_id),
        Some(primary_id),
        "label-matched update must preserve the primary key_id"
    );
    assert_eq!(
        legacy_updated
            .api_keys
            .iter()
            .find(|key| key.key_label == "primary")
            .map(|key| key.api_key.as_str()),
        Some("rotated-primary-secret"),
        "secret rotation must apply to the existing key"
    );
    assert_eq!(
        legacy_updated
            .api_keys
            .iter()
            .find(|key| key.key_label == "secondary")
            .map(|key| key.key_id),
        Some(secondary_id),
        "label-matched update must preserve the secondary key_id"
    );

    let swapped = db::update_endpoint(
        &schema.pool,
        endpoint.endpoint_id,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "key-identity-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://key-identity.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![
                db::EndpointApiKeyCreate {
                    key_label: "secondary".to_string(),
                    api_key: "".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: Some(secondary_id),
                },
                db::EndpointApiKeyCreate {
                    key_label: "renamed-primary".to_string(),
                    api_key: "rotated-primary-secret".to_string(),
                    position: 1,
                    enabled: true,
                    key_id: Some(primary_id),
                },
            ],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?
    .expect("endpoint updated");
    let mut by_position = swapped.api_keys.clone();
    by_position.sort_by_key(|key| key.position);
    assert_eq!(by_position[0].key_id, secondary_id);
    assert_eq!(by_position[0].key_label, "secondary");
    assert_eq!(by_position[0].position, 0);
    assert_eq!(by_position[1].key_id, primary_id);
    assert_eq!(
        by_position[1].key_label, "renamed-primary",
        "explicit key_id must win over the label"
    );
    assert_eq!(by_position[1].position, 1);
    assert_eq!(
        by_position[1].api_key, "rotated-primary-secret",
        "renaming via key_id must keep the stored secret"
    );

    let replaced = db::update_endpoint(
        &schema.pool,
        endpoint.endpoint_id,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "key-identity-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://key-identity.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![db::EndpointApiKeyCreate {
                key_label: "brand-new".to_string(),
                api_key: "brand-new-secret".to_string(),
                position: 0,
                enabled: true,
                key_id: None,
            }],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?
    .expect("endpoint updated");
    assert_eq!(
        replaced.api_keys.len(),
        1,
        "replacing all keys must drop the previous keys"
    );
    assert_eq!(replaced.api_keys[0].key_label, "brand-new");
    assert_eq!(replaced.api_keys[0].api_key, "brand-new-secret");
    assert_ne!(replaced.api_keys[0].key_id, primary_id);
    assert_ne!(replaced.api_keys[0].key_id, secondary_id);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_clears_conversation_binding() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-reset", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "affinity-reset-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://affinity-reset.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "affinity-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-reset".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(Some(admin.user_id), None, None, None)
            .with_route(Some(endpoint.endpoint_id), Some(rule.rule_id))
            .with_model(Some("gpt-affinity-reset".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;

    let cache_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        admin.user_id,
        rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let store = state.replay_cache.response_affinity();
    let binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
    };
    store.get_or_create(&cache_key, &binding).await?;
    assert_eq!(
        store.get(&cache_key).await?,
        Some(binding.clone()),
        "binding should exist before reset"
    );

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["cleared"], true);
    assert_eq!(body["cleared_count"], 1);
    assert_eq!(
        store.get(&cache_key).await?,
        None,
        "binding should be cleared after reset"
    );

    let repeated = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(repeated.into_body(), usize::MAX).await?)?;
    assert_eq!(
        body["cleared"], false,
        "repeated reset must stay idempotent"
    );
    assert_eq!(body["cleared_count"], 0);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_clears_both_record_and_current_rule_bindings() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-both", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "affinity-both-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://affinity-both.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "affinity-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let old_rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-both".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let current_rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-both".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(Some(admin.user_id), None, None, None)
            .with_route(Some(endpoint.endpoint_id), Some(old_rule.rule_id))
            .with_model(Some("gpt-affinity-both".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;

    let store = state.replay_cache.response_affinity();
    let binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
    };
    let old_rule_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        admin.user_id,
        old_rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let current_rule_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        admin.user_id,
        current_rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    store.get_or_create(&old_rule_key, &binding).await?;
    store.get_or_create(&current_rule_key, &binding).await?;
    assert_eq!(store.get(&old_rule_key).await?, Some(binding.clone()));
    assert_eq!(store.get(&current_rule_key).await?, Some(binding.clone()));

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["cleared"], true);
    assert_eq!(body["cleared_count"], 2);
    assert_eq!(
        store.get(&old_rule_key).await?,
        None,
        "binding recorded under the old rule must be cleared"
    );
    assert_eq!(
        store.get(&current_rule_key).await?,
        None,
        "binding resolved under the current rule must be cleared"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_requires_conversation_id() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-noconv", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let record_id = insert_request_record(&schema.pool, admin.user_id, "gpt-test").await?;

    let response = worker_admin::router(state)
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["error"]["code"], "no_conversation_id");

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_requires_admin() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-403", true).await?;
    let user = create_user(&schema.pool, "user-affinity-403", false).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    state
        .replay_cache
        .write_session(
            "non-admin-session",
            &SessionUser {
                user_id: user.user_id,
                login_name: user.login_name.clone(),
                display_name: user.display_name.clone(),
                is_admin: false,
            },
        )
        .await
        .unwrap();
    let record_id = insert_request_record(&schema.pool, user.user_id, "gpt-test").await?;

    let response = worker_admin::router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/admin/request-records/{record_id}/reset-session-affinity"
                ))
                .header(header::COOKIE, "prompt_ferry_session=non-admin-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_returns_503_when_backend_unavailable() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-503", true).await?;
    let replay_cache = ReplayCache::for_tests_without_affinity();
    let state = AdminState::new(AdminStateInit {
        pool: schema.pool.clone(),
        lease_pool: schema.pool.clone(),
        replay_cache: replay_cache.clone(),
        configured_relays: vec!["ws://relay:8788/ws/worker".to_string()],
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
        mcp_catalog_service: McpCatalogService::new(schema.pool.clone(), McpCatalogCache::new()),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        mcp_quota_valkey: prompt_ferry::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: prompt_ferry::endpoint_models::EndpointModelCache::new(
            Duration::from_secs(300),
        ),
    });
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
        .await
        .unwrap();

    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "affinity-503-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://affinity-503.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "affinity-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-503".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(Some(admin.user_id), None, None, None)
            .with_route(Some(endpoint.endpoint_id), Some(rule.rule_id))
            .with_model(Some("gpt-affinity-503".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;

    let response = worker_admin::router(state)
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(
        body["error"]["code"],
        "responses_session_affinity_unavailable"
    );

    schema.cleanup().await?;
    Ok(())
}

fn affinity_fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(secret.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn session_affinity_options_fixture(
    schema: &TestSchema,
    admin: &db::User,
) -> anyhow::Result<(
    AdminState,
    db::ProviderEndpoint,
    db::ModelEndpointRule,
    i64,
    Uuid,
)> {
    let state = admin_state(schema.pool.clone(), admin).await;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "affinity-options-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://affinity-options.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "legacy-key".to_string(),
            api_keys: vec![
                db::EndpointApiKeyCreate {
                    key_label: "primary".to_string(),
                    api_key: "primary-secret".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: None,
                },
                db::EndpointApiKeyCreate {
                    key_label: "rotated".to_string(),
                    api_key: "rotated-secret".to_string(),
                    position: 1,
                    enabled: true,
                    key_id: None,
                },
            ],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-options".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(Some(admin.user_id), None, None, None)
            .with_route(Some(endpoint.endpoint_id), Some(rule.rule_id))
            .with_model(Some("gpt-affinity-options".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;
    Ok((state, endpoint, rule, record_id, conversation_id))
}

async fn session_route_options_affinity(
    state: &AdminState,
    record_id: i64,
) -> anyhow::Result<Value> {
    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "GET",
            format!("/api/v1/admin/request-records/{record_id}/session-route-options"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    Ok(body["affinity"].clone())
}

#[tokio::test]
async fn session_route_options_reports_live_affinity_states() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-options", true).await?;
    let (state, endpoint, rule, record_id, conversation_id) =
        session_affinity_options_fixture(&schema, &admin).await?;
    let primary = endpoint
        .api_keys
        .iter()
        .find(|key| key.key_label == "primary")
        .expect("primary endpoint key");
    let store = state.replay_cache.response_affinity();
    let cache_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        admin.user_id,
        rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );

    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "unbound");
    assert_eq!(affinity["endpoint_id"], Value::Null);

    let active_binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: Some(primary.key_id),
        endpoint_key_fingerprint: affinity_fingerprint("primary-secret"),
    };
    store.get_or_create(&cache_key, &active_binding).await?;
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "active");
    assert_eq!(affinity["rule_id"], rule.rule_id.to_string());
    assert_eq!(affinity["endpoint_id"], endpoint.endpoint_id.to_string());
    assert_eq!(affinity["endpoint_name"], "affinity-options-test");
    assert_eq!(affinity["key_id"], primary.key_id.to_string());
    assert_eq!(affinity["key_label"], "primary");

    let stale_key_binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: affinity_fingerprint("revoked-secret"),
    };
    store.delete(&cache_key).await?;
    store.get_or_create(&cache_key, &stale_key_binding).await?;
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "stale_key");
    assert_eq!(affinity["endpoint_name"], "affinity-options-test");
    assert_eq!(affinity["key_id"], Value::Null);

    let stale_endpoint_binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: Uuid::new_v4(),
        endpoint_key_id: None,
        endpoint_key_fingerprint: affinity_fingerprint("revoked-secret"),
    };
    store.delete(&cache_key).await?;
    store
        .get_or_create(&cache_key, &stale_endpoint_binding)
        .await?;
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "stale_endpoint");
    assert_eq!(
        affinity["endpoint_id"],
        stale_endpoint_binding.endpoint_id.to_string()
    );

    store.delete(&cache_key).await?;
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "unbound");

    store.get_or_create(&cache_key, &active_binding).await?;
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(affinity["state"], "active");
    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["cleared"], true);
    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(
        affinity["state"], "unbound",
        "live affinity must flip to unbound right after reset"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reset_session_affinity_clears_anonymous_record_binding_under_user_zero()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-anon", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "affinity-anon-test".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://affinity-anon.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "affinity-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let rule = db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-affinity-anon".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;
    let conversation_id = Uuid::new_v4();
    let record_id = db::record_request_record(
        &schema.pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_request_actor(None, None, None, None)
            .with_route(Some(endpoint.endpoint_id), Some(rule.rule_id))
            .with_model(Some("gpt-affinity-anon".to_string()))
            .with_request_context(db::RequestRecordContextInput {
                conversation_id: Some(conversation_id),
                parent_event_id: None,
                conversation_seq: Some(1),
                conversation_source: "session_header".to_string(),
                client_installation_id: None,
                normalized_item_count: None,
                normalized_chain_hash: None,
                normalized_first_ref_hash: None,
                normalized_last_ref_hash: None,
                base_checkpoint_event_id: None,
            }),
    )
    .await?;

    let cache_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        0,
        rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let store = state.replay_cache.response_affinity();
    let binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: affinity_fingerprint("affinity-key"),
    };
    store.get_or_create(&cache_key, &binding).await?;
    assert_eq!(store.get(&cache_key).await?, Some(binding.clone()));

    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(
        affinity["state"], "active",
        "anonymous binding under user 0 must be surfaced as live"
    );

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["cleared"], true);
    assert_eq!(body["cleared_count"], 1);
    assert_eq!(
        store.get(&cache_key).await?,
        None,
        "anonymous binding under user 0 must be cleared by the reset"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn session_route_options_surfaces_binding_when_rule_no_longer_resolves() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-affinity-gonerule", true).await?;
    let (state, endpoint, rule, record_id, conversation_id) =
        session_affinity_options_fixture(&schema, &admin).await?;

    let cache_key = prompt_ferry::response_affinity::ResponseAffinityStore::cache_key(
        admin.user_id,
        rule.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let store = state.replay_cache.response_affinity();
    let binding = prompt_ferry::response_affinity::ResponseAffinityBinding {
        endpoint_id: endpoint.endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
    };
    store.get_or_create(&cache_key, &binding).await?;
    assert_eq!(store.get(&cache_key).await?, Some(binding.clone()));
    assert!(
        db::update_model_endpoint_rule(
            &schema.pool,
            rule.rule_id,
            db::ModelEndpointRuleCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                model_pattern: "gpt-some-other-model".to_string(),
                routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
                daily_max_requests: None,
                monthly_max_requests: None,
                enabled: true,
                targets: vec![db::ModelRouteTargetCreate {
                    endpoint_id: endpoint.endpoint_id,
                    enabled: true,
                    upstream_model: None,
                    responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                    chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
                }],
            },
        )
        .await?
        .is_some(),
        "fixture rule must be updated"
    );

    let affinity = session_route_options_affinity(&state, record_id).await?;
    assert_eq!(
        affinity["state"], "stale_endpoint",
        "binding under the recorded rule must be visible even without a resolved route"
    );
    assert_eq!(affinity["endpoint_id"], endpoint.endpoint_id.to_string());

    let response = worker_admin::router(state.clone())
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/request-records/{record_id}/reset-session-affinity"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body["cleared"], true);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn available_models_respects_model_route_whitelist() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-models", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let routed_endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "routed".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://routed.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "routed-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let extra_endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "extra".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://extra.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "extra-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-routed".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: routed_endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;

    let visible_routes = db::list_visible_endpoints(&schema.pool, admin.user_id).await?;
    for route in &visible_routes {
        let model_id = if route.route_id == routed_endpoint.endpoint_id {
            "gpt-routed"
        } else if route.route_id == extra_endpoint.endpoint_id {
            "gpt-extra"
        } else {
            continue;
        };
        state
            .endpoint_model_cache
            .put(
                route,
                prompt_ferry::endpoint_models::EndpointModelSnapshot::from_model_ids([model_id]),
            )
            .await;
    }

    let app = worker_admin::router(state);
    let response = app
        .oneshot(auth_request("GET", "/api/v1/me/models".to_string()))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        value,
        serde_json::json!({
            "models": [{
                "id": "gpt-routed",
                "name": "gpt-routed"
            }]
        })
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn available_models_filters_endpoint_catalog_by_model_patterns() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "available-models-admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "glm".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "http://glm.example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "glm-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "glm-5".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                chat_reasoning_replay_policy: db::ChatReasoningReplayPolicy::Auto,
            }],
        },
    )
    .await?;

    let visible_routes = db::list_visible_endpoints(&schema.pool, admin.user_id).await?;
    for route in &visible_routes {
        if route.route_id == endpoint.endpoint_id {
            state
                .endpoint_model_cache
                .put(
                    route,
                    prompt_ferry::endpoint_models::EndpointModelSnapshot::from_model_ids([
                        "glm-5", "glm-5.1",
                    ]),
                )
                .await;
        }
    }

    let app = worker_admin::router(state);
    let response = app
        .oneshot(auth_request("GET", "/api/v1/me/models".to_string()))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        value,
        serde_json::json!({
            "models": [{
                "id": "glm-5",
                "name": "glm-5"
            }]
        })
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn bridge_status_reports_multi_relay_connectivity() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "bridge-admin", true).await?;
    let replay_cache = ReplayCache::for_tests();
    let state = AdminState::new(AdminStateInit {
        pool: schema.pool.clone(),
        lease_pool: schema.pool.clone(),
        replay_cache: replay_cache.clone(),
        configured_relays: vec![
            "ws://relay-a:8788/ws/worker".to_string(),
            "ws://relay-b:8788/ws/worker".to_string(),
        ],
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
        mcp_catalog_service: McpCatalogService::new(schema.pool.clone(), McpCatalogCache::new()),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        mcp_quota_valkey: prompt_ferry::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: prompt_ferry::endpoint_models::EndpointModelCache::new(
            Duration::from_secs(300),
        ),
    });
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
        .await
        .unwrap();
    let (relay_a_tx, _relay_a_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    let (relay_b_tx, _relay_b_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    worker_admin::set_bridge_sender(&state, "ws://relay-a:8788/ws/worker", Some(relay_a_tx)).await;
    worker_admin::set_bridge_sender(&state, "ws://relay-b:8788/ws/worker", Some(relay_b_tx)).await;

    let app = worker_admin::router(state);
    let response = app
        .oneshot(auth_request("GET", "/api/v1/bridge/status".to_string()))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(value["configured_relays"], 2);
    assert_eq!(value["connected_relays"], 2);
    let relays = value["relays"].as_array().expect("relays array");
    assert_eq!(relays.len(), 2);
    assert_eq!(relays[0]["relay_url"], "ws://relay-a:8788/ws/worker");
    assert_eq!(relays[0]["connected"], true);
    assert_eq!(relays[1]["relay_url"], "ws://relay-b:8788/ws/worker");
    assert_eq!(relays[1]["connected"], true);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn logout_clears_valkey_session() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin-logout", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let app = worker_admin::router(state.clone());
    let logout_response = app
        .oneshot(auth_request("POST", "/api/v1/auth/logout".to_string()))
        .await
        .unwrap();
    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);
    let app = worker_admin::router(state);
    let me_response = app
        .oneshot(auth_request("GET", "/api/v1/auth/me".to_string()))
        .await
        .unwrap();
    assert_eq!(me_response.status(), StatusCode::UNAUTHORIZED);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn reject_endpoint_wakes_waiter_and_clears_payload() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let approval = create_pending_approval(&schema.pool, admin.user_id).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .approval_waiters
        .lock()
        .await
        .insert(approval.approval_id, tx);

    let app = worker_admin::router(state.clone());
    let response = app
        .oneshot(auth_request(
            "POST",
            format!("/api/v1/admin/approvals/{}/reject", approval.approval_id),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), rx)
            .await
            .unwrap()
            .unwrap(),
        ApprovalResolution::Rejected
    );

    let stored = db::get_approval_request(&schema.pool, approval.approval_id)
        .await?
        .unwrap();
    assert_eq!(stored.approval_status, "rejected");
    assert!(stored.request_payload_json.is_none());
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn list_endpoint_filters_pending_and_resolved() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let pending = create_pending_approval(&schema.pool, admin.user_id).await?;
    let resolved = create_pending_approval(&schema.pool, admin.user_id).await?;
    db::resolve_approval_request(
        &schema.pool,
        resolved.approval_id,
        prompt_ferry::llm_review::ApprovalStatus::Rejected,
        Some(admin.user_id),
    )
    .await?;

    let pending_body = to_bytes(
        worker_admin::router(state.clone())
            .oneshot(auth_request(
                "GET",
                "/api/v1/admin/approvals?status=pending&first=0&rows=10".to_string(),
            ))
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await?;
    let pending_json: Value = serde_json::from_slice(&pending_body)?;
    assert_eq!(pending_json.get("total").and_then(Value::as_i64), Some(1));
    assert_eq!(
        pending_json["approvals"][0]["approval_id"].as_str(),
        Some(pending.approval_id.to_string().as_str())
    );

    let resolved_body = to_bytes(
        worker_admin::router(state.clone())
            .oneshot(auth_request(
                "GET",
                "/api/v1/admin/approvals?status=resolved&first=0&rows=10".to_string(),
            ))
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await?;
    let resolved_json: Value = serde_json::from_slice(&resolved_body)?;
    assert_eq!(resolved_json.get("total").and_then(Value::as_i64), Some(1));
    assert_eq!(
        resolved_json["approvals"][0]["approval_id"].as_str(),
        Some(resolved.approval_id.to_string().as_str())
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn usage_facets_endpoint_returns_facets() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    insert_request_record(&schema.pool, admin.user_id, "deepseek-v4-pro").await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let app = worker_admin::router(state.clone());
    let response = app
        .oneshot(auth_request(
            "GET",
            "/api/v1/admin/request-records/facets".to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        value["models"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("deepseek-v4-pro")
    );
    assert_eq!(
        value["users"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("admin")
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn usage_facets_endpoint_returns_mcp_server_facets() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    insert_mcp_request_record(&schema.pool, admin.user_id, "local-tools").await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let app = worker_admin::router(state.clone());
    let response = app
        .oneshot(auth_request(
            "GET",
            "/api/v1/admin/request-records/facets?request_category=mcp".to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        value["models"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("local-tools")
    );
    assert_eq!(
        value["users"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("admin")
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn usage_event_detail_includes_assistant_artifact_fields() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let event_id = insert_request_record(&schema.pool, admin.user_id, "deepseek-v4-pro").await?;
    db::upsert_usage_assistant_artifact(
        &schema.pool,
        db::UsageAssistantArtifactCreate {
            event_id,
            message_json: serde_json::json!({
                "version": 1,
                "assistant_message": {
                    "role": "assistant",
                    "content": "final answer",
                    "reasoning_content": "internal reasoning"
                },
                "output_items": [
                    { "type": "reasoning", "content": [{ "type": "reasoning_text", "text": "internal reasoning" }] },
                    { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "final answer" }] }
                ]
            }),
            has_reasoning_content: true,
            has_tool_calls: false,
        },
    )
    .await?;
    let state = admin_state(schema.pool.clone(), &admin).await;

    let app = worker_admin::router(state.clone());
    let response = app
        .oneshot(auth_request(
            "GET",
            format!("/api/v1/admin/request-records/{event_id}"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let value: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        value["assistant_message_json"]["reasoning_content"].as_str(),
        Some("internal reasoning")
    );
    assert_eq!(
        value["assistant_output_items_json"][0]["type"].as_str(),
        Some("reasoning")
    );
    assert_eq!(value["has_reasoning_content"].as_bool(), Some(true));

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_force_passthrough_for_chat_native_target() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "chat-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "https://chat.example.com".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "secret".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;

    let app = worker_admin::router(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/model-routes")
        .header(header::COOKIE, "prompt_ferry_session=test-session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "scope": "admin",
                "owner_user_id": null,
                "model_pattern": "gpt-test",
                "enabled": true,
                "targets": [{
                    "endpoint_id": endpoint.endpoint_id,
                    "enabled": true,
                    "responses_continuation_policy": "force_passthrough"
                }]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        json["error"]["code"].as_str(),
        Some("invalid_target_continuation_policy")
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn defaults_force_passthrough_for_responses_native_target() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let state = admin_state(schema.pool.clone(), &admin).await;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            base_url: "https://responses.example.com".to_string(),
            native_api: NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "secret".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;

    let app = worker_admin::router(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/model-routes")
        .header(header::COOKIE, "prompt_ferry_session=test-session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "scope": "admin",
                "owner_user_id": null,
                "model_pattern": "gpt-test",
                "enabled": true,
                "targets": [{
                    "endpoint_id": endpoint.endpoint_id,
                    "enabled": true
                }]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&body)?;
    assert_eq!(
        json["targets"][0]["responses_continuation_policy"].as_str(),
        Some("force_passthrough")
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn abort_pending_helper_marks_records_aborted() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping approval api test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    let admin = create_user(&schema.pool, "admin", true).await?;
    let approval = create_pending_approval(&schema.pool, admin.user_id).await?;

    let affected = db::abort_pending_approval_requests(&schema.pool).await?;
    assert_eq!(affected, 1);

    let stored = db::get_approval_request(&schema.pool, approval.approval_id)
        .await?
        .unwrap();
    assert_eq!(stored.approval_status, "aborted");
    assert!(stored.request_payload_json.is_none());
    schema.cleanup().await?;
    Ok(())
}
