use std::{env, str::FromStr};

use prompt_ferry::db;
use sqlx::{
    Executor, PgPool,
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
    schema.cleanup().await?;
    Ok(())
}
