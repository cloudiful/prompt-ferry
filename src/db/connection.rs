use anyhow::{Context, Result};
use db_init::{DbInitOptions, connect_pool, run_migrations};
use sqlx::{
    PgPool, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions,
};
use std::{path::Path, time::Duration};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
static STANDALONE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/standalone");

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const SQLITE_MAX_CONNECTIONS: u32 = 4;

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

pub async fn connect_sqlite(path: impl AsRef<Path>) -> sqlx::Result<SqlitePool> {
    connect_sqlite_with_max_connections(path, SQLITE_MAX_CONNECTIONS).await
}

pub async fn connect_sqlite_with_max_connections(
    path: impl AsRef<Path>,
    max_connections: u32,
) -> sqlx::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .pragma("journal_mode", "WAL");
    SqlitePoolOptions::new()
        .max_connections(max_connections.max(1))
        .connect_with(options)
        .await
}

pub async fn migrate_standalone(pool: &SqlitePool) -> Result<()> {
    STANDALONE_MIGRATOR
        .run(pool)
        .await
        .context("failed to run standalone SQLite migrations")
}
