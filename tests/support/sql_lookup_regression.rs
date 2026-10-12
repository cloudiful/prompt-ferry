use chrono::Utc;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// The dedicated quota test database, reached only after
/// `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` and its confirmed target agree.
/// Unlike the skippable quota helpers this file fails outright when neither is
/// configured, so a missing setting is never reported as passing coverage.
pub(super) async fn isolated_pool() -> PgPool {
    test_pg_target::required_pool(1)
        .await
        .expect("connect to the confirmed quota test database")
}

pub(super) async fn insert_endpoint(connection: &mut PgConnection, endpoint_id: Uuid, name: &str) {
    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
          native_api, native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, 'generic', NULL, 'https://example.invalid/v1', \
                 'responses', 'manual', 'sql-lookup-fixture-key', TRUE)",
    )
    .bind(endpoint_id)
    .bind(name)
    .execute(connection)
    .await
    .expect("insert endpoint fixture");
}

pub(super) async fn insert_request(
    connection: &mut PgConnection,
    request_id: Uuid,
    user_id: Option<i64>,
    endpoint_id: Option<Uuid>,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO request_records (request_id, user_id, endpoint_id, path, created_at) \
         VALUES ($1, $2, $3, '/v1/responses', $4) RETURNING event_id",
    )
    .bind(request_id)
    .bind(user_id)
    .bind(endpoint_id)
    .bind(Utc::now())
    .fetch_one(connection)
    .await
    .expect("insert request fixture")
}

pub(super) async fn insert_charge(
    connection: &mut PgConnection,
    event_id: i64,
    request_id: Uuid,
    user_id: Option<i64>,
    endpoint_id: Option<Uuid>,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO usage_charges \
         (event_id, request_id, user_id, endpoint_id, usage_status, pricing_status, currency) \
         VALUES ($1, $2, $3, $4, 'known', 'priced', 'CNY') RETURNING charge_id",
    )
    .bind(event_id)
    .bind(request_id)
    .bind(user_id)
    .bind(endpoint_id)
    .fetch_one(connection)
    .await
    .expect("insert charge fixture")
}
