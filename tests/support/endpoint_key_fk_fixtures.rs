//! Owned endpoints, keys and users for the endpoint key FK index tests.
//!
//! Every row here belongs to one fixture, identified by the endpoint's own
//! UUID, and cleanup deletes by that exact id. Nothing sweeps by name or by
//! prefix: the database holds endpoints and charges this suite did not create,
//! and a fixture that cannot prove ownership must not delete.

use sqlx::{PgPool, Row};
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
pub async fn pool(max_connections: u32) -> PgPool {
    test_pg_target::required_pool(max_connections)
        .await
        .expect("connect to the confirmed quota test database")
}

/// One endpoint and the key the FK rows will point at.
pub struct Owned {
    pub endpoint_id: Uuid,
    pub key_id: Uuid,
    pub user_id: i64,
}

/// Create an endpoint holding exactly one key, plus the user that owns it.
///
/// The key is what the foreign keys reference, so deleting it is what drives
/// the `ON DELETE SET NULL` scan these tests measure.
pub async fn owned_endpoint(pool: &PgPool, label: &str) -> Owned {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let user_id = {
        let row = sqlx::query(
            "INSERT INTO users (login_name, password_hash, display_name, is_admin)              VALUES ($1, 'fixture', 'fixture', TRUE) RETURNING user_id",
        )
        .bind(format!("fkidx-{label}-{}", Uuid::new_v4().simple()))
        .fetch_one(pool)
        .await
        .expect("insert the fixture user");
        row.get::<i64, _>("user_id")
    };

    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, base_url, native_api, \
          native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, 'generic', 'https://example.invalid/v1', \
                 'responses', 'manual', 'fixture-secret', TRUE)",
    )
    .bind(endpoint_id)
    .bind(format!("fkidx-{label}-{}", Uuid::new_v4().simple()))
    .execute(pool)
    .await
    .expect("insert the fixture endpoint");

    sqlx::query(
        "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, enabled) \
         VALUES ($1, $2, 'fixture-key', 'fixture-secret', 0, TRUE)",
    )
    .bind(key_id)
    .bind(endpoint_id)
    .execute(pool)
    .await
    .expect("insert the fixture endpoint key");

    Owned {
        endpoint_id,
        key_id,
        user_id,
    }
}

/// Whether an index exists and whether PostgreSQL considers it usable.
///
/// `indisvalid` is the one that matters operationally: an index left invalid by
/// a failed concurrent build still exists in the catalog but is neither trusted
/// nor used, so a report that only counted names would call that "present".
pub async fn index_state(pool: &PgPool, index_name: &str) -> Option<IndexState> {
    let row = sqlx::query(
        "SELECT i.indisvalid, i.indisready, pg_get_indexdef(i.indexrelid) AS definition \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indexrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relname = $1 AND n.nspname = current_schema()",
    )
    .bind(index_name)
    .fetch_optional(pool)
    .await
    .expect("read the index catalog entry")?;

    Some(IndexState {
        valid: row.get("indisvalid"),
        ready: row.get("indisready"),
        definition: row.get("definition"),
    })
}

pub struct IndexState {
    pub valid: bool,
    pub ready: bool,
    pub definition: String,
}

/// Whether a lookup on `table.column` can use an index named `index_name`.
///
/// The check is on the catalog rather than on a plan: a plan is whatever the
/// planner chose for the data present, and a seq scan on a small table is a
/// correct answer to a cheap question, not a missing index.
pub async fn index_leads_with(pool: &PgPool, index_name: &str, column: &str) -> bool {
    let row = sqlx::query(
        "SELECT a.attname AS column_name, k.ord AS position \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indexrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
         WHERE c.relname = $1 AND n.nspname = current_schema() AND k.ord = 1",
    )
    .bind(index_name)
    .fetch_optional(pool)
    .await
    .expect("read the leading index column");

    row.is_some_and(|row| row.get::<String, _>("column_name") == column)
}

/// Remove exactly the rows this fixture created, in dependency order.
pub async fn cleanup(pool: &PgPool, owned: &Owned) {
    sqlx::query("DELETE FROM usage_charges WHERE endpoint_id = $1")
        .bind(owned.endpoint_id)
        .execute(pool)
        .await
        .expect("remove the fixture charges");
    sqlx::query("DELETE FROM conversation_endpoint_overrides WHERE endpoint_id = $1")
        .bind(owned.endpoint_id)
        .execute(pool)
        .await
        .expect("remove the fixture overrides");
    sqlx::query("DELETE FROM endpoint_api_keys WHERE endpoint_id = $1")
        .bind(owned.endpoint_id)
        .execute(pool)
        .await
        .expect("remove the fixture key");
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(owned.endpoint_id)
        .execute(pool)
        .await
        .expect("remove the fixture endpoint");
    sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(owned.user_id)
        .execute(pool)
        .await
        .expect("remove the fixture user");
}
