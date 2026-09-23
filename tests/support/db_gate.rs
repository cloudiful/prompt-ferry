//! Shared PostgreSQL plumbing for the migration and DB integration suites.
//!
//! Two problems are solved here, both about the throwaway `pfy_test_*` schemas
//! the DB suites create in the shared test database:
//!
//! * `MigrationGate` serializes `db::migrate` calls. The migration set contains
//!   a `-- no-transaction` `CREATE INDEX CONCURRENTLY` (`20260923093701`). SQLx
//!   already serializes migrations with a session advisory lock, but a task
//!   that *blocks* on that lock keeps a transaction open, and an index build
//!   waits for every transaction holding an older snapshot. The holder then
//!   waits on the blocked waiters, which wait on the holder: a real deadlock
//!   that failed the suite with `deadlock detected`. The gate is taken with
//!   non-blocking `pg_try_advisory_lock` polling, so a waiter never parks on an
//!   old snapshot while another migration builds the index.
//! * `SchemaCleanup` covers the residue: a test that leaves early (a panic or a
//!   `?`) skipped its explicit `cleanup`, and abandoned schemas piled up in the
//!   shared database. It drops the schema on `Drop` unless the test already
//!   cleaned up, so no failure path leaks one.

use std::{
    env,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgConnectOptions};

pub const TEST_DATABASE_URL_ENV: &str = "PROMPT_FERRY_TEST_DATABASE_URL";

/// Namespace for the per-database gate; kept separate from the bigint key
/// space SQLx uses for its own migration lock so the two never collide.
const GATE_NAMESPACE: &str = "prompt_ferry_test_migration_gate";
const GATE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(600);
const GATE_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Exclusive per-database gate held while a test schema runs `db::migrate`.
///
/// The gate owns a dedicated connection: dropping it ends the session, and
/// PostgreSQL releases session-level advisory locks when the backend exits.
pub struct MigrationGate {
    connection: PgConnection,
}

impl MigrationGate {
    pub async fn acquire() -> anyhow::Result<Self> {
        let database_url = env::var(TEST_DATABASE_URL_ENV)?;
        let options = PgConnectOptions::from_str(&database_url)?;
        let mut connection = PgConnection::connect_with(&options).await?;
        let deadline = std::time::Instant::now() + GATE_ACQUIRE_TIMEOUT;
        loop {
            let acquired = sqlx::query_scalar::<_, bool>(
                "SELECT pg_try_advisory_lock(hashtext($1), hashtext(current_database()))",
            )
            .bind(GATE_NAMESPACE)
            .fetch_one(&mut connection)
            .await?;
            if acquired {
                return Ok(Self { connection });
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "timed out after {}s waiting for the test migration gate",
                    GATE_ACQUIRE_TIMEOUT.as_secs()
                );
            }
            tokio::time::sleep(GATE_POLL_INTERVAL).await;
        }
    }
}

impl Drop for MigrationGate {
    fn drop(&mut self) {
        // Closing this connection (socket close) ends the session, and the
        // server drops the session-level advisory lock with the backend.
        let _ = &self.connection;
    }
}

const DROP_SCHEMA_LOCK_TIMEOUT: &str = "2s";
const DROP_SCHEMA_ATTEMPTS: usize = 15;

const LOCKER_PIDS_SQL: &str = r#"
    SELECT DISTINCT locks.pid
    FROM pg_locks locks
    JOIN pg_class cls ON cls.oid = locks.relation
    JOIN pg_namespace nsp ON nsp.oid = cls.relnamespace
    WHERE nsp.nspname = $1
      AND locks.pid <> pg_backend_pid()
"#;

/// Terminates every backend still holding a lock inside `schema`.
///
/// Aborting a test worker can leave one of its pooled connections with an open
/// transaction; such a session makes `DROP SCHEMA` wait forever, so teardown
/// stops those sessions instead of waiting for them.
async fn terminate_lockers(
    connection: &mut PgConnection,
    schema: &str,
) -> Result<i64, sqlx::Error> {
    let sql = format!("SELECT COUNT(pg_terminate_backend(pid)) FROM ({LOCKER_PIDS_SQL}) lockers");
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(schema)
        .fetch_one(connection)
        .await
}

/// Bounded, locker-tolerant `DROP SCHEMA` using a pooled connection.
pub async fn drop_schema_robust_pool(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
    let mut connection = pool.acquire().await?;
    drop_schema_robust(&mut connection, schema).await;
    Ok(())
}

/// Drops `schema`, terminating any lingering locker if the first attempt is
/// blocked; bounded so a stray session can never hang a teardown forever.
async fn drop_schema_robust(connection: &mut PgConnection, schema: &str) {
    let _ = connection
        .execute(sqlx::AssertSqlSafe(format!(
            "SET lock_timeout = '{DROP_SCHEMA_LOCK_TIMEOUT}'"
        )))
        .await;
    let drop_sql = format!(r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#);
    for _ in 0..DROP_SCHEMA_ATTEMPTS {
        match connection
            .execute(sqlx::AssertSqlSafe(drop_sql.clone()))
            .await
        {
            Ok(_) => return,
            Err(_) => {
                let _ = terminate_lockers(connection, schema).await;
            }
        }
    }
}

/// Drops a throwaway schema on a dedicated connection, blocking until done.
///
/// `Drop` cannot await and the schema must be gone before the process moves on,
/// so the work runs on its own thread with its own runtime.
pub fn drop_schema_blocking(schema: &str) {
    let Ok(database_url) = env::var(TEST_DATABASE_URL_ENV) else {
        return;
    };
    let schema = schema.to_string();
    let cleanup = std::thread::Builder::new()
        .name("pfy-test-schema-cleanup".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let Ok(options) = PgConnectOptions::from_str(&database_url) else {
                    return;
                };
                let Ok(mut connection) = PgConnection::connect_with(&options).await else {
                    return;
                };
                drop_schema_robust(&mut connection, &schema).await;
            });
        });
    if let Ok(handle) = cleanup {
        let _ = handle.join();
    }
}

/// Drops the schema on `Drop` unless the test already cleaned it up.
pub struct SchemaCleanup {
    schema: String,
    armed: AtomicBool,
}

impl SchemaCleanup {
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            armed: AtomicBool::new(true),
        }
    }

    /// Marks the schema as already dropped by the explicit `cleanup` path.
    pub fn disarm(&self) {
        self.armed.store(false, Ordering::Relaxed);
    }
}

impl Drop for SchemaCleanup {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Relaxed) {
            return;
        }
        drop_schema_blocking(&self.schema);
    }
}
