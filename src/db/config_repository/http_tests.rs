use std::{path::PathBuf, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    db::{self, ConfigRepository},
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    standalone_config::StandaloneConfigStore,
    worker_admin,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};

fn test_manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("test manager")
}

#[test]
fn postgres_uuid_compatibility_uses_the_legacy_id_half() {
    assert_eq!(
        super::postgres_key_id(Uuid::from_u64_pair(42, 0)).unwrap(),
        42
    );
    assert!(super::postgres_key_id(Uuid::from_u64_pair(i64::MAX as u64 + 1, 0)).is_err());
}

async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-client-keys-{}.sqlite",
        Uuid::new_v4()
    ));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let user_store = db::UserStore::sqlite(store.pool().clone());
    user_store
        .bootstrap_admin("admin", "admin-password")
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
        mcp_quota_valkey: crate::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: EndpointModelCache::new(std::time::Duration::from_secs(60)),
    })
    .with_user_store(user_store)
    .with_config_repository(ConfigRepository::sqlite(store.clone(), test_manager()));
    state
        .replay_cache
        .write_session(
            "client-key-test",
            &SessionUser {
                user_id: 1,
                login_name: "admin".to_string(),
                display_name: "Admin".to_string(),
                is_admin: true,
            },
        )
        .await
        .expect("session");
    (state, store, path)
}

fn request(method: &str, uri: String, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, "prompt_ferry_session=client-key-test");
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body"),
    )
    .expect("JSON response")
}

#[tokio::test]
async fn sqlite_http_client_keys_round_trip_uuid_identifiers() -> anyhow::Result<()> {
    let (state, store, path) = test_state().await;
    let first = state
        .config_repository
        .create_client_key(1, Some("first"), true)
        .await?;
    let second = state
        .config_repository
        .create_client_key(1, Some("second"), true)
        .await?;
    let third = state
        .config_repository
        .create_client_key(1, Some("third"), true)
        .await?;
    let app = worker_admin::router(state.clone());

    let listed = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/admin/users/1/client-keys".to_string(),
            None,
        ))
        .await?;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    let listed_keys = listed_body["keys"].as_array().expect("keys");
    let listed_ids = listed_keys
        .iter()
        .map(|key| key["key_id"].as_str().expect("UUID key ID").to_string())
        .collect::<Vec<_>>();
    assert!(listed_ids.contains(&second.key.key_id.to_string()));
    let first_index = listed_ids
        .iter()
        .position(|key_id| key_id == &first.key.key_id.to_string())
        .expect("first key in ordered list");

    // Admin list response must surface the persisted synthetic secret for
    // every key without re-encrypting it through any unrelated endpoint.
    let admin_secrets: std::collections::HashMap<String, String> = listed_keys
        .iter()
        .map(|key| {
            (
                key["key_id"].as_str().expect("UUID key ID").to_string(),
                key["secret"].as_str().expect("secret string").to_string(),
            )
        })
        .collect();
    assert_eq!(admin_secrets[&first.key.key_id.to_string()], first.secret);
    assert_eq!(admin_secrets[&second.key.key_id.to_string()], second.secret);
    assert_eq!(admin_secrets[&third.key.key_id.to_string()], third.secret);

    let legacy_updated = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/users/1/client-keys/{first_index}"),
            Some(serde_json::json!({"label": "first-legacy"})),
        ))
        .await?;
    assert_eq!(legacy_updated.status(), StatusCode::OK);
    let legacy_updated_body = json_body(legacy_updated).await;
    assert_eq!(legacy_updated_body["key_id"], first.key.key_id.to_string());
    assert_eq!(legacy_updated_body["label"], "first-legacy");
    assert_eq!(
        legacy_updated_body["secret"]
            .as_str()
            .expect("legacy secret"),
        first.secret
    );

    let updated = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/users/1/client-keys/{}", second.key.key_id),
            Some(serde_json::json!({"label": "second-updated"})),
        ))
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = json_body(updated).await;
    assert_eq!(updated_body["key_id"], second.key.key_id.to_string());
    assert_eq!(updated_body["label"], "second-updated");
    assert_eq!(
        updated_body["secret"].as_str().expect("updated secret"),
        second.secret
    );

    let me_listed = app
        .clone()
        .oneshot(request("GET", "/api/v1/me/client-keys".to_string(), None))
        .await?;
    assert_eq!(me_listed.status(), StatusCode::OK);
    let me_listed_body = json_body(me_listed).await;
    let me_listed_keys = me_listed_body["keys"].as_array().expect("me keys");
    let me_secrets: std::collections::HashMap<String, String> = me_listed_keys
        .iter()
        .map(|key| {
            (
                key["key_id"].as_str().expect("UUID key ID").to_string(),
                key["secret"].as_str().expect("secret string").to_string(),
            )
        })
        .collect();
    assert_eq!(me_secrets[&first.key.key_id.to_string()], first.secret);
    assert_eq!(me_secrets[&second.key.key_id.to_string()], second.secret);
    assert_eq!(me_secrets[&third.key.key_id.to_string()], third.secret);

    let me_updated = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/me/client-keys/{}", third.key.key_id),
            Some(serde_json::json!({"enabled": false})),
        ))
        .await?;
    assert_eq!(me_updated.status(), StatusCode::OK);
    let me_updated_body = json_body(me_updated).await;
    assert_eq!(me_updated_body["key_id"], third.key.key_id.to_string());
    assert_eq!(me_updated_body["enabled"], false);
    assert_eq!(
        me_updated_body["secret"]
            .as_str()
            .expect("me updated secret"),
        third.secret
    );

    let invalid = app
        .clone()
        .oneshot(request(
            "DELETE",
            "/api/v1/admin/users/1/client-keys/not-a-client-key".to_string(),
            None,
        ))
        .await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["error"]["code"], "bad_request");

    let deleted = app
        .oneshot(request(
            "DELETE",
            format!("/api/v1/admin/users/1/client-keys/{}", first.key.key_id),
            None,
        ))
        .await?;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let (_, remaining) = state
        .config_repository
        .list_client_keys_page(1, 0, 10)
        .await?;
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().all(|key| key.key_id != first.key.key_id));
    let second_after = remaining
        .iter()
        .find(|key| key.key_id == second.key.key_id)
        .expect("second key remains");
    assert_eq!(second_after.label, "second-updated");
    let third_after = remaining
        .iter()
        .find(|key| key.key_id == third.key.key_id)
        .expect("third key remains");
    assert!(!third_after.enabled);

    drop(state);
    let pool = store.pool().clone();
    drop(store);
    pool.close().await;
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[tokio::test]
async fn sqlite_raw_object_store_is_explicitly_unsupported() -> anyhow::Result<()> {
    let (state, store, path) = test_state().await;
    let app = worker_admin::router(state.clone());

    let get = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/settings/raw-object-store".to_string(),
            None,
        ))
        .await?;
    assert_eq!(get.status(), StatusCode::NOT_IMPLEMENTED);
    let body = json_body(get).await;
    assert_eq!(
        body["error"]["code"].as_str().expect("code"),
        "sqlite_raw_object_store_unavailable"
    );

    let patch = app
        .oneshot(request(
            "PATCH",
            "/api/v1/settings/raw-object-store".to_string(),
            Some(serde_json::json!({
                "backend": "local",
                "local_dir": "",
                "s3_endpoint": "",
                "s3_bucket": "",
                "s3_region": "auto",
                "s3_prefix": "prompt-ferry/raw",
                "s3_allow_http": false,
                "s3_access_key": null,
                "s3_secret_key": null
            })),
        ))
        .await?;
    assert_eq!(patch.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        json_body(patch).await["error"]["code"]
            .as_str()
            .expect("code"),
        "sqlite_raw_object_store_unavailable"
    );

    drop(state);
    let pool = store.pool().clone();
    drop(store);
    pool.close().await;
    let _ = std::fs::remove_file(path);
    Ok(())
}
