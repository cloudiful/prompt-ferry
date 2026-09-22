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
}

impl TestSchema {
    pub async fn new() -> anyhow::Result<Self> {
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
            pool,
            admin_pool,
            schema_name,
        })
    }

    pub async fn cleanup(&self) -> anyhow::Result<()> {
        // A test that aborts its worker can leave worker-spawned background
        // tasks polling the schema; their locks make `DROP SCHEMA CASCADE`
        // deadlock. Terminate those backends first, then retry the drop.
        let drop_statement = format!(r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#, self.schema_name);
        for remaining in (0..5).rev() {
            self.terminate_schema_backends().await?;
            match self
                .admin_pool
                .execute(sqlx::AssertSqlSafe(drop_statement.clone()))
                .await
            {
                Ok(_) => break,
                Err(error) if is_deadlock(&error) && remaining > 0 => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.pool.close().await;
        self.admin_pool.close().await;
        Ok(())
    }

    async fn terminate_schema_backends(&self) -> anyhow::Result<()> {
        sqlx::query(include_str!("../sql/schema_locked_backend_pids.sql"))
            .bind(&self.schema_name)
            .execute(&self.admin_pool)
            .await?;
        Ok(())
    }
}

fn is_deadlock(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .is_some_and(|code| code.as_ref() == "40P01")
}

pub fn test_database_configured() -> bool {
    env::var(TEST_DATABASE_URL_ENV).is_ok()
}
