//! Endpoints, keys and cleanup for the endpoint key persistence tests.
//!
//! Every fixture owns the UUIDs it creates and removes only those rows. The
//! shared database also holds endpoints this suite did not create, so cleanup
//! is scoped to the endpoint under test and never sweeps the table.
//!
//! Key secrets are never read back here: the assertions compare identity,
//! ordering and timestamps, and holding a secret would mean a failing
//! assertion could print one.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db::{EndpointApiKeyCreate, EndpointCreate, EndpointProvider},
};

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// The dedicated quota test database, reached only after
/// `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` and its confirmed target agree.
/// A missing setting fails the test rather than reporting skipped coverage.
pub async fn pool(max_connections: u32) -> PgPool {
    test_pg_target::required_pool(max_connections)
        .await
        .expect("connect to the confirmed quota test database")
}

/// One stored key, as far as these tests care about it.
#[derive(Clone)]
pub struct StoredKey {
    pub key_id: Uuid,
    pub key_label: String,
    pub position: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The stored keys of one endpoint, in their persisted order.
pub async fn stored_keys(pool: &PgPool, endpoint_id: Uuid) -> Vec<StoredKey> {
    let rows = sqlx::query(
        "SELECT key_id, key_label, position, enabled, created_at, updated_at \
         FROM endpoint_api_keys WHERE endpoint_id = $1 \
         ORDER BY position ASC, created_at ASC",
    )
    .bind(endpoint_id)
    .fetch_all(pool)
    .await
    .expect("read stored endpoint keys");

    rows.into_iter()
        .map(|row| StoredKey {
            key_id: row.get("key_id"),
            key_label: row.get("key_label"),
            position: row.get("position"),
            enabled: row.get("enabled"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect()
}

/// The endpoint row's own name, to show whether a write survived.
pub async fn stored_endpoint_name(pool: &PgPool, endpoint_id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT name FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .fetch_optional(pool)
        .await
        .expect("read endpoint name")
}

/// How many endpoints carry this name.
pub async fn endpoints_named(pool: &PgPool, name: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM provider_endpoints WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("count endpoints")
}

/// A distinct endpoint name per fixture, so two suites never collide.
pub fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

/// The endpoint body these tests create and update.
pub fn endpoint_input(name: &str, api_keys: Vec<EndpointApiKeyCreate>) -> EndpointCreate {
    EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: name.to_string(),
        provider: EndpointProvider::Generic,
        provider_region: None,
        service_tier: None,
        base_url: "https://example.invalid/v1".to_string(),
        native_api: NativeApi::Responses,
        native_api_source: NativeApiSource::Manual,
        api_key: "fixture-endpoint-secret".to_string(),
        api_keys,
        key_lb_enabled: false,
        enabled: true,
        proxy_url: None,
        active_windows: None,
    }
}

/// A requested key. `key_id` is set only when the caller means an existing key.
pub fn key(
    key_id: Option<Uuid>,
    key_label: &str,
    position: i32,
    enabled: bool,
) -> EndpointApiKeyCreate {
    EndpointApiKeyCreate {
        key_label: key_label.to_string(),
        api_key: format!("{key_label}-secret"),
        position,
        enabled,
        key_id,
    }
}

/// Remove the endpoint this fixture created, and with it its keys.
///
/// The delete cascades to the keys and sets the foreign keys that point at them
/// to NULL, so nothing outside the endpoint is left dangling.
pub async fn cleanup(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove the endpoint this test created");
}
