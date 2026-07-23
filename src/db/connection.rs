use anyhow::{Context, Result};
use db_init::{DbInitOptions, connect_pool, run_migrations};
use sqlx::{PgPool, migrate::Migrator};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn connect(database_url: &str) -> Result<PgPool> {
    connect_with_max_connections(database_url, 8).await
}

pub async fn connect_with_max_connections(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool> {
    connect_pool(database_url, DbInitOptions { max_connections })
        .await
        .context("failed to connect postgres")
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    run_migrations(pool, &MIGRATOR)
        .await
        .context("failed to run database migrations")
}
