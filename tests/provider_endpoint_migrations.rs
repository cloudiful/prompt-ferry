use std::{env, str::FromStr};

use prompt_ferry::{db, standalone_config::StandaloneConfigStore};
use sha2::{Digest, Sha384};
use sqlx::{
    Executor, PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
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

#[tokio::test]
async fn migrate_adds_provider_endpoint_protocol_columns() -> anyhow::Result<()> {
    if env::var(TEST_DATABASE_URL_ENV).is_err() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE provider_endpoints (
                endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
                owner_user_id BIGINT,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    let row =
        sqlx::query_file!("tests/sql/provider_endpoint_migrations/provider_endpoint_defaults.sql")
            .fetch_one(&schema.pool)
            .await?;

    assert!(row.native_api_default.contains("'chat'"));
    assert!(row.native_api_source_default.contains("'manual'"));
    assert!(row.provider_default.contains("'generic'"));
    schema.cleanup().await?;
    Ok(())
}

async fn insert_provider_endpoint(
    pool: &PgPool,
    name: &str,
    provider: &str,
    region: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO provider_endpoints (scope, name, base_url, api_key, provider, provider_region)
           VALUES ('admin', $1, 'https://example.test', 'secret', $2, $3)"#,
    )
    .bind(name)
    .bind(provider)
    .bind(region)
    .execute(pool)
    .await
    .map(|_| ())
}

// 0070 up: command_code behaves like generic (NULL region) while minimax
// keeps its cn/global requirement.
#[tokio::test]
async fn migrate_0070_command_code_provider_up_enforces_region_shape() -> anyhow::Result<()> {
    if env::var(TEST_DATABASE_URL_ENV).is_err() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE provider_endpoints (
                endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
                owner_user_id BIGINT,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    insert_provider_endpoint(&schema.pool, "cc-null", "command_code", None).await?;
    insert_provider_endpoint(&schema.pool, "cc-region", "command_code", Some("cn"))
        .await
        .expect_err("command_code must not carry a provider region");
    insert_provider_endpoint(&schema.pool, "mm-cn", "minimax", Some("cn")).await?;
    insert_provider_endpoint(&schema.pool, "mm-global", "minimax", Some("global")).await?;
    insert_provider_endpoint(&schema.pool, "mm-null", "minimax", None)
        .await
        .expect_err("minimax requires a provider region");
    insert_provider_endpoint(&schema.pool, "gen-null", "generic", None).await?;
    insert_provider_endpoint(&schema.pool, "gen-region", "generic", Some("cn"))
        .await
        .expect_err("generic must not carry a provider region");

    schema.cleanup().await?;
    Ok(())
}

// 0070 down: command_code rows fold back to generic and the narrower
// CHECKs apply again.
#[tokio::test]
async fn migrate_0070_down_folds_command_code_back_to_generic() -> anyhow::Result<()> {
    if env::var(TEST_DATABASE_URL_ENV).is_err() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE provider_endpoints (
                endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
                owner_user_id BIGINT,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;
    insert_provider_endpoint(&schema.pool, "cc-row", "command_code", None).await?;

    sqlx::raw_sql(include_str!(
        "../migrations/0070_command_code_provider.down.sql"
    ))
    .execute(&schema.pool)
    .await?;

    let row =
        sqlx::query("SELECT provider, provider_region FROM provider_endpoints WHERE name = 'cc-row'")
            .fetch_one(&schema.pool)
            .await?;
    assert_eq!(row.try_get::<String, _>("provider")?, "generic");
    assert!(
        row.try_get::<Option<String>, _>("provider_region")?
            .is_none()
    );
    insert_provider_endpoint(&schema.pool, "cc-after-down", "command_code", None)
        .await
        .expect_err("command_code is rejected after the 0070 down migration");
    insert_provider_endpoint(&schema.pool, "gen-after-down", "generic", None).await?;

    schema.cleanup().await?;
    Ok(())
}

fn standalone_temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "pfy-cmdcode-{}-{}-{}.sqlite",
        tag,
        std::process::id(),
        Uuid::new_v4().simple()
    ))
}

fn remove_standalone_files(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let candidate = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            path.with_extension(format!("sqlite{suffix}"))
        };
        let _ = std::fs::remove_file(candidate);
    }
    let _ = std::fs::remove_file(path);
}

async fn standalone_schema_version(pool: &sqlx::SqlitePool) -> anyhow::Result<i64> {
    let row = sqlx::query(
        "SELECT schema_version FROM standalone_schema_meta WHERE schema_key = 'standalone'",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.try_get::<i64, _>("schema_version")?)
}

async fn insert_standalone_endpoint(
    pool: &sqlx::SqlitePool,
    name: &str,
    provider: &str,
    region: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO standalone_provider_endpoints
           (endpoint_id, name, provider, provider_region, base_url, native_api,
            native_api_source, key_lb_enabled, enabled, mcp_enabled,
            api_key_ciphertext, api_key_nonce, api_key_key_version)
           VALUES (?, ?, ?, ?, 'https://example.test', 'responses', 'manual',
                   0, 1, 0, X'00', X'01', 1)"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(name)
    .bind(provider)
    .bind(region)
    .execute(pool)
    .await
    .map(|_| ())
}

// Standalone 0014 fresh path: a new store migrates to schema 14 with the
// provider CHECK widened to command_code, opencode_go and openrouter.
#[tokio::test]
async fn standalone_0014_fresh_migration_supports_command_code_opencode_go_and_openrouter() -> anyhow::Result<()> {
    let path = standalone_temp_path("fresh");
    let store = StandaloneConfigStore::open(&path).await?;
    let pool = db::connect_sqlite(&path).await?;
    assert_eq!(standalone_schema_version(&pool).await?, 14);

    let ddl: String = sqlx::query(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'standalone_provider_endpoints'",
    )
    .fetch_one(&pool)
    .await?
    .try_get(0)?;
    assert!(
        ddl.contains("command_code"),
        "provider CHECK must list command_code: {ddl}"
    );
    assert!(
        ddl.contains("opencode_go"),
        "provider CHECK must list opencode_go: {ddl}"
    );
    assert!(
        ddl.contains("openrouter"),
        "provider CHECK must list openrouter: {ddl}"
    );

    insert_standalone_endpoint(&pool, "cc-fresh", "command_code", None).await?;
    insert_standalone_endpoint(&pool, "og-fresh", "opencode_go", None).await?;
    insert_standalone_endpoint(&pool, "or-fresh", "openrouter", None).await?;
    insert_standalone_endpoint(&pool, "bogus-fresh", "legacy-unknown", None)
        .await
        .expect_err("unknown providers stay rejected");

    pool.close().await;
    store.close().await;
    remove_standalone_files(&path);
    Ok(())
}

// Standalone 0014 upgrade path: a v13 database keeps its rows (including
// command_code and opencode_go) and gains openrouter after open() applies
// the pending migration, while openrouter stays rejected at schema 13.
#[tokio::test]
async fn standalone_0014_upgrade_from_v13_preserves_rows_and_widens_provider() -> anyhow::Result<()>
{
    const APPLIED: [(i64, &str, &str); 13] = [
        (1, "0001_initial", include_str!("../migrations/standalone/0001_initial.sql")),
        (2, "0002_storage_contract", include_str!("../migrations/standalone/0002_storage_contract.sql")),
        (3, "0003_user_auth_compatibility", include_str!("../migrations/standalone/0003_user_auth_compatibility.sql")),
        (4, "0004_coordinator_state", include_str!("../migrations/standalone/0004_coordinator_state.sql")),
        (5, "0005_mcp_configuration", include_str!("../migrations/standalone/0005_mcp_configuration.sql")),
        (6, "0006_request_ledger", include_str!("../migrations/standalone/0006_request_ledger.sql")),
        (7, "0007_request_metadata", include_str!("../migrations/standalone/0007_request_metadata.sql")),
        (8, "0008_replay_snapshots", include_str!("../migrations/standalone/0008_replay_snapshots.sql")),
        (9, "0009_request_leases", include_str!("../migrations/standalone/0009_request_leases.sql")),
        (10, "0010_mcp_basic_auth", include_str!("../migrations/standalone/0010_mcp_basic_auth.sql")),
        (11, "0011_minimax_service_tier", include_str!("../migrations/standalone/0011_minimax_service_tier.sql")),
        (12, "0012_command_code_provider", include_str!("../migrations/standalone/0012_command_code_provider.sql")),
        (13, "0013_opencode_go_provider", include_str!("../migrations/standalone/0013_opencode_go_provider.sql")),
    ];
    let path = standalone_temp_path("upgrade");
    let pool = db::connect_sqlite(&path).await?;
    for (_, version, body) in APPLIED {
        sqlx::raw_sql(body)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{version}: {error}"));
    }
    insert_standalone_endpoint(&pool, "legacy-minimax", "minimax", Some("cn")).await?;
    insert_standalone_endpoint(&pool, "legacy-cc", "command_code", None).await?;
    insert_standalone_endpoint(&pool, "legacy-og", "opencode_go", None).await?;
    insert_standalone_endpoint(&pool, "legacy-or", "openrouter", None)
        .await
        .expect_err("openrouter is rejected at schema 13");
    assert_eq!(standalone_schema_version(&pool).await?, 13);

    // Record the manually applied migrations so open() only applies 0014,
    // mirroring the crate-internal upgrade tests.
    sqlx::raw_sql(
        r#"CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN NOT NULL,
            checksum BLOB NOT NULL,
            execution_time BIGINT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await?;
    for (version, description, body) in &APPLIED {
        let checksum = Sha384::digest(body.as_bytes()).to_vec();
        sqlx::query(
            "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES (?, ?, 1, ?, 0)",
        )
        .bind(*version)
        .bind(*description)
        .bind(checksum)
        .execute(&pool)
        .await?;
    }
    pool.close().await;

    let store = StandaloneConfigStore::open(&path).await?;
    let pool = db::connect_sqlite(&path).await?;
    assert_eq!(standalone_schema_version(&pool).await?, 14);
    let preserved: i64 =
        sqlx::query("SELECT COUNT(*) FROM standalone_provider_endpoints WHERE name = 'legacy-minimax'")
            .fetch_one(&pool)
            .await?
            .try_get(0)?;
    assert_eq!(preserved, 1, "v13 rows must survive the 0014 rebuild");
    let preserved_cc: i64 =
        sqlx::query("SELECT COUNT(*) FROM standalone_provider_endpoints WHERE name = 'legacy-cc'")
            .fetch_one(&pool)
            .await?
            .try_get(0)?;
    assert_eq!(preserved_cc, 1, "command_code rows must survive the v13->v14 rebuild");
    let preserved_og: i64 =
        sqlx::query("SELECT COUNT(*) FROM standalone_provider_endpoints WHERE name = 'legacy-og'")
            .fetch_one(&pool)
            .await?
            .try_get(0)?;
    assert_eq!(preserved_og, 1, "opencode_go rows must survive the v13->v14 rebuild");
    insert_standalone_endpoint(&pool, "cc-upgraded", "command_code", None).await?;
    insert_standalone_endpoint(&pool, "og-upgraded", "opencode_go", None).await?;
    insert_standalone_endpoint(&pool, "or-upgraded", "openrouter", None).await?;

    pool.close().await;
    store.close().await;
    remove_standalone_files(&path);
    Ok(())
}
