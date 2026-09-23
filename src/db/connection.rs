use anyhow::{Context, Result, anyhow};
use db_init::run_migrations;
use sqlx::{
    PgPool, SqlitePool, migrate::Migrator, postgres::PgPoolOptions, sqlite::SqliteConnectOptions,
    sqlite::SqlitePoolOptions,
};
use std::{env, path::Path, time::Duration};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
static STANDALONE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/standalone");

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const SQLITE_MAX_CONNECTIONS: u32 = 4;
/// Bounded wait for the initial PostgreSQL handshake. Without this
/// wrapper sqlx uses the OS TCP connect timeout (~60-120s) and the
/// worker hangs in startup long after the orchestrator has moved on.
pub const POSTGRES_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Cap on waiting for a pooled PostgreSQL connection. The sqlx default is
/// 30s, which let an exhausted pool stall its callers for the whole wait
/// before reporting `PoolTimedOut`.
pub const POSTGRES_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
/// Keep one connection warm without paying for an eagerly opened full pool.
const POSTGRES_MIN_CONNECTIONS: u32 = 1;
const POSTGRES_ACQUIRE_TIMEOUT_ENV: &str = "PROMPT_FERRY_DB_POOL_ACQUIRE_TIMEOUT_SECONDS";
const POSTGRES_MIN_CONNECTIONS_ENV: &str = "PROMPT_FERRY_DB_POOL_MIN_CONNECTIONS";

pub async fn connect(database_url: &str) -> Result<PgPool> {
    connect_with_max_connections(database_url, 8).await
}

/// Build a PostgreSQL pool with an explicit acquire timeout.
///
/// `db_init::connect_pool` cannot express `acquire_timeout`, so the options
/// are built here; the handshake timeout wrapper is unchanged.
pub async fn connect_with_max_connections(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool> {
    let attempt = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(postgres_min_connections(max_connections))
        .acquire_timeout(postgres_acquire_timeout())
        .connect(database_url);
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

/// Pool acquire bound, overridable with
/// `PROMPT_FERRY_DB_POOL_ACQUIRE_TIMEOUT_SECONDS`; malformed or zero values
/// keep the default so a typo cannot remove the bound.
fn postgres_acquire_timeout() -> Duration {
    resolve_acquire_timeout(env::var(POSTGRES_ACQUIRE_TIMEOUT_ENV).ok())
}

fn resolve_acquire_timeout(configured: Option<String>) -> Duration {
    configured
        .as_deref()
        .and_then(parse_acquire_timeout_seconds)
        .map(Duration::from_secs)
        .unwrap_or(POSTGRES_ACQUIRE_TIMEOUT)
}

fn parse_acquire_timeout_seconds(raw: &str) -> Option<u64> {
    raw.trim()
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
}

/// Minimum kept-warm connections, overridable with
/// `PROMPT_FERRY_DB_POOL_MIN_CONNECTIONS` and clamped to the pool size.
fn postgres_min_connections(max_connections: u32) -> u32 {
    resolve_min_connections(env::var(POSTGRES_MIN_CONNECTIONS_ENV).ok(), max_connections)
}

fn resolve_min_connections(configured: Option<String>, max_connections: u32) -> u32 {
    configured
        .as_deref()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(POSTGRES_MIN_CONNECTIONS)
        .min(max_connections)
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

#[cfg(test)]
mod tests {
    use super::{
        POSTGRES_ACQUIRE_TIMEOUT, POSTGRES_MIN_CONNECTIONS, resolve_acquire_timeout,
        resolve_min_connections,
    };
    use std::time::Duration;

    #[test]
    fn acquire_timeout_defaults_to_five_seconds_and_accepts_an_env_override() {
        assert_eq!(resolve_acquire_timeout(None), POSTGRES_ACQUIRE_TIMEOUT);
        assert_eq!(
            resolve_acquire_timeout(Some("12".to_string())),
            Duration::from_secs(12)
        );
        assert_eq!(
            resolve_acquire_timeout(Some("  7  ".to_string())),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn malformed_or_zero_acquire_timeouts_keep_the_default() {
        for configured in ["", "0", "-3", "soon"] {
            assert_eq!(
                resolve_acquire_timeout(Some(configured.to_string())),
                POSTGRES_ACQUIRE_TIMEOUT,
                "`{configured}` must keep the bounded default"
            );
        }
    }

    #[test]
    fn min_connections_defaults_small_and_clamps_to_the_pool_size() {
        assert_eq!(resolve_min_connections(None, 8), POSTGRES_MIN_CONNECTIONS);
        assert_eq!(
            resolve_min_connections(Some("4".to_string()), 8),
            4,
            "an explicit override below the pool size is honoured"
        );
        assert_eq!(
            resolve_min_connections(Some("99".to_string()), 4),
            4,
            "an override above the pool size is clamped"
        );
        assert_eq!(
            resolve_min_connections(Some("nonsense".to_string()), 4),
            POSTGRES_MIN_CONNECTIONS
        );
        assert_eq!(
            resolve_min_connections(Some("2".to_string()), 0),
            0,
            "a degenerate pool size must not invert the min/max relationship"
        );
    }
}
