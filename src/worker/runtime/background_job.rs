//! Shared plumbing for long-running scheduled background jobs (raw payload
//! maintenance, cache alert monitor): the coordinated/local scheduler runners,
//! the Valkey health probe used by coordinated runs, and the PostgreSQL
//! advisory lease taken when no Valkey coordination is configured.

use super::lifecycle::RuntimeControl;
use redis::{AsyncCommands, aio::ConnectionManager};
use scheduler::{
    CoordinatedLeaseConfig, InMemoryStateStore, NoopExecutionGuard, Scheduler, SchedulerConfig,
    SchedulerError, ValkeyCoordinatedStateStore,
};
use sqlx::{PgPool, Postgres, pool::PoolConnection};
use std::time::Duration;
use tokio::time::timeout;

const VALKEY_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(15);
const VALKEY_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const VALKEY_HEALTH_FAILURE_LIMIT: usize = 3;
const VALKEY_HEALTH_RETRY_BACKOFF: Duration = Duration::from_secs(1);
const COORDINATED_LEASE_TTL: Duration = Duration::from_secs(30 * 60);
const COORDINATED_LEASE_RENEW: Duration = Duration::from_secs(60);

pub(super) type CoordinatedScheduler =
    Scheduler<InMemoryStateStore, NoopExecutionGuard, ValkeyCoordinatedStateStore>;

/// Connect the shared Valkey coordination store, or `None` when it is
/// unreachable inside the bounded probe so the caller can disable the job
/// instead of running it uncoordinated.
pub(super) async fn connect_coordinated_store(
    valkey_url: &str,
) -> Option<ValkeyCoordinatedStateStore> {
    match timeout(
        VALKEY_HEALTH_CHECK_TIMEOUT,
        ValkeyCoordinatedStateStore::new(valkey_url),
    )
    .await
    {
        Ok(Ok(store)) => Some(store),
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error,
                capability = "job_coordination",
                "background job disabled because Valkey coordination is unavailable"
            );
            None
        }
        Err(_) => {
            tracing::warn!(
                capability = "job_coordination",
                "background job disabled because Valkey coordination initialization timed out"
            );
            None
        }
    }
}

pub(super) fn coordinated_scheduler(store: ValkeyCoordinatedStateStore) -> CoordinatedScheduler {
    Scheduler::with_coordinated_state_store(
        SchedulerConfig::default(),
        store,
        CoordinatedLeaseConfig {
            ttl: COORDINATED_LEASE_TTL,
            renew_interval: COORDINATED_LEASE_RENEW,
        },
    )
}

pub(super) async fn run_scheduler<S, G, C, D>(
    scheduler: Scheduler<S, G, C>,
    job: scheduler::Job<D>,
    control: RuntimeControl,
) -> Result<(), SchedulerError>
where
    S: scheduler::StateStore + Send + Sync + 'static,
    G: scheduler::ExecutionGuard + Send + Sync + 'static,
    C: scheduler::CoordinatedStateStore + Send + Sync + 'static,
    D: Send + Sync + 'static,
{
    let handle = scheduler.handle();
    let run = scheduler.run(job);
    tokio::pin!(run);

    tokio::select! {
        result = &mut run => result.map(|_| ()),
        _ = control.wait_for_shutdown() => {
            handle.shutdown();
            (&mut run).await.map(|_| ())
        }
    }
}

pub(super) async fn run_coordinated_scheduler<D>(
    scheduler: CoordinatedScheduler,
    job: scheduler::Job<D>,
    valkey_url: String,
    control: RuntimeControl,
) -> Result<(), String>
where
    D: Send + Sync + 'static,
{
    let handle = scheduler.handle();
    let run = run_scheduler(scheduler, job, control.clone());
    tokio::pin!(run);
    let health = monitor_valkey_health(valkey_url);
    tokio::pin!(health);

    tokio::select! {
        result = &mut run => result.map(|_| ()).map_err(|error| error.to_string()),
        _ = control.wait_for_shutdown() => {
            handle.shutdown();
            (&mut run).await.map(|_| ()).map_err(|error| error.to_string())
        }
        result = &mut health => {
            let error = match result {
                Ok(()) => "Valkey health monitor stopped unexpectedly".to_string(),
                Err(error) => error.to_string(),
            };
            handle.shutdown();
            let _ = (&mut run).await;
            Err(error)
        }
    }
}

async fn monitor_valkey_health(url: String) -> anyhow::Result<()> {
    let client = redis::Client::open(url.as_str())?;
    let mut manager: ConnectionManager =
        timeout(VALKEY_HEALTH_CHECK_TIMEOUT, client.get_connection_manager())
            .await
            .map_err(|_| {
                anyhow::anyhow!("timed out connecting to Valkey for scheduler health check")
            })??;

    loop {
        tokio::time::sleep(VALKEY_HEALTH_CHECK_INTERVAL).await;
        let mut backoff = VALKEY_HEALTH_RETRY_BACKOFF;
        let mut last_error = None;
        let mut healthy = false;
        for attempt in 0..VALKEY_HEALTH_FAILURE_LIMIT {
            match timeout(VALKEY_HEALTH_CHECK_TIMEOUT, manager.ping()).await {
                Ok(Ok(())) => {
                    healthy = true;
                    break;
                }
                Ok(Err(error)) => last_error = Some(error.to_string()),
                Err(_) => {
                    last_error = Some("Valkey health check timed out".to_string());
                }
            }
            if attempt + 1 < VALKEY_HEALTH_FAILURE_LIMIT {
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2);
            }
        }
        if !healthy {
            return Err(anyhow::anyhow!(
                "Valkey scheduler health check failed after {} attempts: {}",
                VALKEY_HEALTH_FAILURE_LIMIT,
                last_error.unwrap_or_else(|| "unknown error".to_string())
            ));
        }
    }
}

/// Held PostgreSQL advisory lock that keeps a background job single-flight
/// across workers sharing the same database when Valkey coordination is not
/// configured.
pub(super) struct PostgresMaintenanceLease {
    connection: PoolConnection<Postgres>,
    lock_key: i64,
}

impl PostgresMaintenanceLease {
    pub(super) async fn release(mut self) -> anyhow::Result<()> {
        sqlx::query_file_unchecked!(
            "src/sql/standalone/postgres_release_advisory_lock.sql",
            self.lock_key
        )
        .fetch_one(&mut *self.connection)
        .await?;
        Ok(())
    }
}

pub(super) async fn try_postgres_advisory_lease(
    pool: &PgPool,
    lock_key: i64,
) -> anyhow::Result<Option<PostgresMaintenanceLease>> {
    let mut connection = pool.acquire().await?;
    let row = sqlx::query_file!(
        "src/sql/standalone/postgres_try_advisory_lock.sql",
        lock_key
    )
    .fetch_one(&mut *connection)
    .await?;
    if row.acquired.unwrap_or(false) {
        Ok(Some(PostgresMaintenanceLease {
            connection,
            lock_key,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VALKEY_HEALTH_CHECK_TIMEOUT, VALKEY_HEALTH_FAILURE_LIMIT, VALKEY_HEALTH_RETRY_BACKOFF,
    };

    #[test]
    fn scheduler_health_retry_is_bounded() {
        assert_eq!(VALKEY_HEALTH_FAILURE_LIMIT, 3);
        assert!(VALKEY_HEALTH_RETRY_BACKOFF < VALKEY_HEALTH_CHECK_TIMEOUT);
    }
}
