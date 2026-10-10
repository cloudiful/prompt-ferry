use std::{str::FromStr, sync::Arc};

use chrono::Utc;
use serde_json::json;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

use crate::{
    db::{self, ChatgptQuotaSnapshotCreate, ChatgptQuotaSnapshotSource, ConfigRepository},
    worker_admin::{
        chatgpt_quota_service::{ChatGptQuotaFetcher, ChatGptQuotaService},
        handlers::chatgpt_backend::{ChatgptQuota, ChatgptQuotaWindow},
    },
};

pub(crate) async fn test_pool() -> Option<PgPool> {
    let url = match std::env::var("PROMPT_FERRY_QUOTA_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!(
                "skipped ChatGPT quota service DB setup: PROMPT_FERRY_QUOTA_TEST_DATABASE_URL is unset"
            );
            return None;
        }
    };
    let parsed = PgConnectOptions::from_str(&url)
        .unwrap_or_else(|_| panic!("quota service test database URL is invalid"));
    assert_eq!(parsed.get_host(), "127.0.0.1");
    assert_eq!(parsed.get_database(), Some("quota_759_test"));
    let options = PgConnectOptions::new_without_pgpass()
        .host("127.0.0.1")
        .port(parsed.get_port())
        .username(parsed.get_username())
        .database("quota_759_test");
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap_or_else(|_| panic!("quota service test database connection failed")),
    )
}

pub(crate) async fn create_endpoint(pool: &PgPool) -> Uuid {
    let endpoint_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
          native_api, native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, 'openai', NULL, 'https://example.invalid/v1', \
                 'responses', 'manual', 'quota-service-fixture-key', true)",
    )
    .bind(endpoint_id)
    .bind(format!("quota-service-{endpoint_id}"))
    .execute(pool)
    .await
    .expect("insert isolated service endpoint");
    sqlx::query(
        "INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token) \
         VALUES ($1, 'fixture-access', 'fixture-refresh')",
    )
    .bind(endpoint_id)
    .execute(pool)
    .await
    .expect("insert fake OAuth presence for isolated endpoint");
    endpoint_id
}

pub(crate) async fn endpoint(pool: &PgPool, endpoint_id: Uuid) -> db::ProviderEndpoint {
    db::get_endpoint(pool, endpoint_id)
        .await
        .expect("load isolated service endpoint")
        .expect("service endpoint exists")
}

pub(crate) async fn cleanup_orphan_fixtures(pool: &PgPool) {
    sqlx::query(
        "DELETE FROM request_records WHERE endpoint_id IN (\
             SELECT endpoint_id FROM provider_endpoints \
             WHERE name LIKE 'quota-service-%' OR name LIKE 'quota-test-%'\
         )",
    )
    .execute(pool)
    .await
    .expect("remove stale isolated quota test activity");
    sqlx::query(
        "DELETE FROM provider_endpoints \
         WHERE name LIKE 'quota-service-%' OR name LIKE 'quota-test-%'",
    )
    .execute(pool)
    .await
    .expect("remove stale isolated quota test endpoints");
}

pub(crate) async fn cleanup_endpoint(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query("DELETE FROM request_records WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove isolated request activity");
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove isolated service endpoint");
}

pub(crate) fn service(pool: &PgPool, fetcher: Arc<dyn ChatGptQuotaFetcher>) -> ChatGptQuotaService {
    ChatGptQuotaService::with_fetcher(pool.clone(), ConfigRepository::postgres(pool), fetcher)
}

pub(crate) fn quota(used_percent: Option<f64>) -> ChatgptQuota {
    ChatgptQuota {
        plan_type: Some("plus".to_string()),
        limit_reached: Some(false),
        primary: Some(ChatgptQuotaWindow {
            used_percent,
            limit_window_seconds: Some(604_800),
            reset_after_seconds: None,
            reset_at: None,
        }),
        secondary: None,
        has_credits: None,
        unlimited_credits: None,
        credits_balance: None,
    }
}

pub(crate) async fn seed_snapshot(
    pool: &PgPool,
    endpoint_id: Uuid,
    observed_at: chrono::DateTime<Utc>,
    used_percent: Option<f64>,
) {
    let attempted_at = Utc::now();
    let owner = Uuid::new_v4();
    assert!(
        db::quota_snapshots::acquire_refresh_lease(
            pool,
            endpoint_id,
            owner,
            attempted_at,
            attempted_at + chrono::Duration::seconds(45),
        )
        .await
        .expect("acquire seed lease")
    );
    assert!(
        db::quota_snapshots::complete_refresh_success(
            pool,
            owner,
            ChatgptQuotaSnapshotCreate {
                endpoint_id,
                observed_at,
                plan_type: Some("plus".to_string()),
                limit_reached: Some(false),
                windows: json!([{
                    "source_window": "primary",
                    "window_seconds": 604_800,
                    "used_percent": used_percent,
                    "remaining_percent": used_percent.map(|used| 100.0 - used),
                    "availability": if used_percent.is_some() { "known" } else { "unknown" }
                }]),
                source: ChatgptQuotaSnapshotSource::Manual,
            },
        )
        .await
        .expect("persist seed observation")
    );
}
