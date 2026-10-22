// Issue #1032 P1: shared PostgreSQL fixtures for the embeddings capability.
//
// Every fixture runs inside one transaction the test rolls back, so a run
// never leaks a row into the dedicated test database. The pool itself is only
// obtained after `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` and its confirmed
// target agree (`test_pg_target::required_pool`), so a missing service is a
// failure instead of a silent skip.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// The confirmed dedicated test database, reached only after the test URL and
/// the confirmed target agree. Fails outright when neither is configured.
pub(super) async fn isolated_pool() -> PgPool {
    test_pg_target::required_pool(1)
        .await
        .expect("connect to the confirmed quota test database")
}

/// Insert one admin endpoint with the given `native_api`. Admin scope keeps
/// `owner_user_id` NULL, so no user fixture is required.
pub(super) async fn insert_endpoint(
    connection: &mut PgConnection,
    endpoint_id: Uuid,
    name: &str,
    native_api: &str,
) {
    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
          native_api, native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, 'generic', NULL, 'https://example.invalid/v1', \
                 $3, 'manual', 'embedding-capability-fixture-key', TRUE)",
    )
    .bind(endpoint_id)
    .bind(name)
    .bind(native_api)
    .execute(connection)
    .await
    .expect("insert endpoint fixture");
}

/// Insert one enabled admin rule with a `*` pattern.
pub(super) async fn insert_rule(
    connection: &mut PgConnection,
    rule_id: Uuid,
    endpoint_id: Uuid,
    model_pattern: &str,
) {
    sqlx::query(
        "INSERT INTO model_endpoint_rules \
         (rule_id, scope, owner_user_id, model_pattern, endpoint_id, routing_strategy, enabled) \
         VALUES ($1, 'admin', NULL, $2, $3, 'responses_session_affinity', TRUE)",
    )
    .bind(rule_id)
    .bind(model_pattern)
    .bind(endpoint_id)
    .execute(connection)
    .await
    .expect("insert rule fixture");
}

/// Insert one enabled target with an optional dimension override.
pub(super) async fn insert_target(
    connection: &mut PgConnection,
    target_id: Uuid,
    rule_id: Uuid,
    endpoint_id: Uuid,
    position: i32,
    dimensions: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO model_route_targets \
         (target_id, rule_id, endpoint_id, position, enabled, native_api, \
          embedding_dimensions_override) \
         VALUES ($1, $2, $3, $4, TRUE, 'embeddings', $5)",
    )
    .bind(target_id)
    .bind(rule_id)
    .bind(endpoint_id)
    .bind(position)
    .bind(dimensions)
    .execute(connection)
    .await
    .map(|_| ())
}
