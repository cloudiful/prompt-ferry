use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    db::{self, ChatgptQuotaSnapshotCreate, ChatgptQuotaSnapshotSource, ConfigRepository},
    worker_admin::{
        chatgpt_quota_service::{ChatGptQuotaFetcher, ChatGptQuotaService},
        handlers::chatgpt_backend::{ChatgptQuota, ChatgptQuotaWindow},
    },
};

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// Only an absent pair of dedicated keys skips. A configured target that is
/// malformed or disagrees with its confirmed identity fails here rather than
/// degrading into a silent skip.
pub(crate) async fn test_pool() -> Option<PgPool> {
    match test_pg_target::optional_pool(4).await {
        Ok(Some(pool)) => Some(pool),
        Ok(None) => {
            eprintln!(
                "skipped ChatGPT quota service DB setup: neither {} nor {} is configured",
                test_pg_target::DATABASE_URL_KEY,
                test_pg_target::CONFIRMED_TARGET_KEY
            );
            None
        }
        Err(error) => panic!("quota service test target is not usable: {error}"),
    }
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
    let endpoint_id = Uuid::parse_str("34dbe486-898c-4965-8790-f1643aec2e7d")
        .expect("known review fixture UUID is valid");
    let expected_name = format!("quota-service-{endpoint_id}");
    let name = sqlx::query_scalar::<_, String>(
        "SELECT name FROM provider_endpoints WHERE endpoint_id = $1",
    )
    .bind(endpoint_id)
    .fetch_optional(pool)
    .await
    .expect("verify exact review fixture before cleanup");
    if name.as_deref() == Some(expected_name.as_str()) {
        cleanup_endpoint(pool, endpoint_id).await;
    } else if name.is_some() {
        panic!("refusing cleanup because exact fixture ownership did not match");
    }
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
