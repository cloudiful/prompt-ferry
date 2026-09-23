//! Regression test for issue 277 P4: the PostgreSQL pool must bound how long
//! a caller waits for a pooled connection instead of inheriting the 30s sqlx
//! default, and the bound must be env-overridable.
//!
//! The test needs no schema objects, so it connects to the configured test
//! database directly and skips when none is configured.

use std::{
    env,
    time::{Duration, Instant},
};

use prompt_ferry::db;

const TEST_DATABASE_URL_ENV: &str = "PROMPT_FERRY_TEST_DATABASE_URL";
const ACQUIRE_TIMEOUT_ENV: &str = "PROMPT_FERRY_DB_POOL_ACQUIRE_TIMEOUT_SECONDS";

#[tokio::test]
async fn exhausted_pool_fails_within_the_configured_acquire_timeout() -> anyhow::Result<()> {
    let Ok(database_url) = env::var(TEST_DATABASE_URL_ENV) else {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    };

    // This is the only test in the binary, so it owns the process environment.
    unsafe { env::set_var(ACQUIRE_TIMEOUT_ENV, "1") };
    let pool = db::connect_with_max_connections(&database_url, 1).await?;
    assert_eq!(
        pool.options().get_acquire_timeout(),
        Duration::from_secs(1),
        "the env override must reach the pool options"
    );

    let held = pool.acquire().await?;
    let started = Instant::now();
    let error = pool
        .acquire()
        .await
        .expect_err("a pool with its only connection checked out must time out");
    let waited = started.elapsed();
    drop(held);
    pool.close().await;

    assert!(
        matches!(error, sqlx::Error::PoolTimedOut),
        "expected PoolTimedOut, got {error:?}"
    );
    assert!(
        waited >= Duration::from_millis(900) && waited < Duration::from_secs(10),
        "the acquire must wait only for the configured bound, not the sqlx default: {waited:?}"
    );

    unsafe { env::remove_var(ACQUIRE_TIMEOUT_ENV) };
    let pool = db::connect_with_max_connections(&database_url, 1).await?;
    assert_eq!(
        pool.options().get_acquire_timeout(),
        Duration::from_secs(5),
        "without an override the pool keeps the 5s default"
    );
    pool.close().await;
    Ok(())
}
