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
        // A worker pool that is still draining a query holds locks on schema
        // objects while `DROP SCHEMA ... CASCADE` takes ACCESS EXCLUSIVE locks on
        // every one of them, so the two deadlock once a schema carries enough
        // partitions. Retry until the lingering backend drains.
        let drop_sql = format!(r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#, self.schema_name);
        let mut attempts = 0;
        loop {
            match self
                .admin_pool
                .execute(sqlx::AssertSqlSafe(drop_sql.clone()))
                .await
            {
                Ok(_) => break,
                Err(err) if is_deadlock(&err) && attempts < 20 => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(err) => return Err(err.into()),
            }
        }
        self.pool.close().await;
        self.admin_pool.close().await;
        Ok(())
    }
}

fn is_deadlock(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("40P01")
}

pub fn test_database_configured() -> bool {
    env::var(TEST_DATABASE_URL_ENV).is_ok()
}
