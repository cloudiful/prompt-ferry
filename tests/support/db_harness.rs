#[path = "db_gate.rs"]
mod db_gate;

use std::{env, str::FromStr};

use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

pub const TEST_DATABASE_URL_ENV: &str = "PROMPT_FERRY_TEST_DATABASE_URL";

pub struct TestSchema {
    pub pool: PgPool,
    admin_pool: PgPool,
    pub schema_name: String,
    cleanup: db_gate::SchemaCleanup,
    /// Held for the whole test so migrations (including the concurrent index
    /// build) and the worker's startup migration never race another test's.
    _gate: db_gate::MigrationGate,
}

impl TestSchema {
    pub async fn new() -> anyhow::Result<Self> {
        let gate = db_gate::MigrationGate::acquire().await?;
        let database_url = env::var(TEST_DATABASE_URL_ENV)?;
        let schema_name = format!("pfy_test_{}", Uuid::new_v4().simple());
        let base_options = PgConnectOptions::from_str(&database_url)?;
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(base_options.clone())
            .await?;
        admin_pool
            .execute(sqlx::AssertSqlSafe(format!(
                r#"CREATE SCHEMA "{}""#,
                schema_name
            )))
            .await?;

        let schema_options = base_options.options([("search_path", schema_name.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(schema_options)
            .await?;

        Ok(Self {
            cleanup: db_gate::SchemaCleanup::new(schema_name.clone()),
            _gate: gate,
            pool,
            admin_pool,
            schema_name,
        })
    }

    pub async fn cleanup(&self) -> anyhow::Result<()> {
        db_gate::drop_schema_robust_pool(&self.admin_pool, &self.schema_name).await?;
        self.cleanup.disarm();
        self.pool.close().await;
        self.admin_pool.close().await;
        Ok(())
    }
}

pub fn test_database_configured() -> bool {
    env::var(TEST_DATABASE_URL_ENV).is_ok()
}
