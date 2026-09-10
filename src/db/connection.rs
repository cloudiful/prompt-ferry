use anyhow::{Context, Result, anyhow};
use db_init::{DbInitOptions, connect_pool, run_migrations};
use sqlx::{
    PgPool, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions,
};
use std::{path::Path, time::Duration};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
static STANDALONE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/standalone");

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const SQLITE_MAX_CONNECTIONS: u32 = 4;
/// Bounded wait for the initial PostgreSQL handshake. Without this
/// wrapper sqlx uses the OS TCP connect timeout (~60-120s) and the
/// worker hangs in startup long after the orchestrator has moved on.
pub const POSTGRES_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn connect(database_url: &str) -> Result<PgPool> {
    connect_with_max_connections(database_url, 8).await
}

pub async fn connect_with_max_connections(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool> {
    let attempt = connect_pool(database_url, DbInitOptions { max_connections });
    match tokio::time::timeout(POSTGRES_CONNECT_TIMEOUT, attempt).await {
        Ok(result) => result.context("failed to connect postgres"),
        Err(_) => Err(anyhow!(
            "postgres connect timed out after {}s (database_url={}); \
             check that the database is reachable and accepting connections",
            POSTGRES_CONNECT_TIMEOUT.as_secs(),
            redact_database_url(database_url),
        )),
    }
}

/// Strip any embedded password so connection failures do not leak
/// credentials into the error chain. Anything we cannot parse falls
/// back to a marker rather than the raw URL.
fn redact_database_url(database_url: &str) -> String {
    let Some(scheme_end) = database_url.find("://") else {
        return "<unparseable>".to_string();
    };
    let (scheme, rest) = database_url.split_at(scheme_end + 3);
    let Some(at_pos) = rest.rfind('@') else {
        return database_url.to_string();
    };
    let (userinfo, host_part) = rest.split_at(at_pos);
    let host = &host_part[1..];
    let user = userinfo.split(':').next().unwrap_or("");
    if user.is_empty() {
        format!("{scheme}{host_part}")
    } else {
        format!("{scheme}{user}:***@{host}")
    }
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
