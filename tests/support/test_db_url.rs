// Test-only PostgreSQL URL helpers.
//
// Every helper here is pure: it builds a URL or a pool handle pinned by an
// explicit `search_path` to a fresh isolated `pfy_test_*` schema, and none of
// them connects. A test that accidentally queries therefore fails on the
// missing schema instead of silently touching the shared `public` schema.

use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

const TEST_SCHEMA_PREFIX: &str = "pfy_test_";

/// A fresh, unique schema name inside the test namespace.
pub fn new_test_schema_name() -> String {
    format!("{TEST_SCHEMA_PREFIX}{}", Uuid::new_v4().simple())
}

/// Rejects anything that is not an isolated `pfy_test_<32 hex digits>` schema.
///
/// This is the guard that keeps any pooled query out of `public` and every
/// business schema.
pub fn assert_test_schema(schema: &str) -> anyhow::Result<()> {
    let valid = schema
        .strip_prefix(TEST_SCHEMA_PREFIX)
        .is_some_and(|rest| rest.len() == 32 && rest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if !valid {
        anyhow::bail!(
            "refusing to use non-test schema {schema:?}; expected {TEST_SCHEMA_PREFIX}<32 hex digits>"
        );
    }
    Ok(())
}

/// Scopes `base` to `schema` via an explicit `search_path`.
pub fn scoped_connect_options(
    base: PgConnectOptions,
    schema: &str,
) -> anyhow::Result<PgConnectOptions> {
    assert_test_schema(schema)?;
    Ok(base.options([("search_path", schema)]))
}

fn append_search_path(url: &str, schema: &str) -> String {
    let search_path = format!("-csearch_path={schema}");
    let option = urlencoding::encode(&search_path);
    let join = if url.contains('?') { "&" } else { "?" };
    format!("{url}{join}options={option}")
}

/// A `DATABASE_URL` copy scoped to `schema` with an explicit `search_path`.
pub fn scoped_database_url(url: &str, schema: &str) -> anyhow::Result<String> {
    assert_test_schema(schema)?;
    Ok(append_search_path(url, schema))
}

/// A schema-scoped URL for config values and lazy pools that never connect.
///
/// It carries only the scheme and the explicit `search_path` for a fresh
/// isolated test schema; there is no host, user, or database because the
/// connection is never attempted.
pub fn lazy_test_database_url() -> String {
    let schema = new_test_schema_name();
    append_search_path("postgres://", &schema)
}

/// A lazy pool scoped to a brand-new `pfy_test_*` schema.
///
/// It is built from libpq environment defaults, so it needs no `DATABASE_URL`
/// and never connects during construction. A stray query fails on the missing
/// isolated schema instead of silently touching `public`.
pub fn lazy_test_pool() -> PgPool {
    let schema = new_test_schema_name();
    let options = scoped_connect_options(PgConnectOptions::new(), &schema)
        .expect("generated test schema name is always valid");
    PgPoolOptions::new().connect_lazy_with(options)
}
