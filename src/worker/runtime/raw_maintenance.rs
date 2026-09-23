use super::background_job::{
    connect_coordinated_store, coordinated_scheduler, run_coordinated_scheduler, run_scheduler,
    try_postgres_advisory_lease,
};
use super::lifecycle::RuntimeControl;
use crate::raw_payload_store::RawPayloadStore;
use crate::worker_admin_types::UsageRetentionSettings;
use crate::{config::WorkerConfig, db};
use rand::RngExt;
use scheduler::{InMemoryStateStore, Job, Schedule, Scheduler, SchedulerConfig, Task, TaskContext};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{info, warn};

const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(15 * 60);
const MAINTENANCE_INITIAL_DELAY_MIN: Duration = Duration::from_secs(5 * 60);
const MAINTENANCE_INITIAL_DELAY_MAX: Duration = Duration::from_secs(15 * 60);
const MAINTENANCE_SCHEDULER_JOB_ID: &str = "prompt-ferry:partition-maintenance";

#[derive(Clone)]
struct MaintenanceDependencies {
    pool: PgPool,
    retention: Arc<RwLock<UsageRetentionSettings>>,
    raw_store: Arc<RwLock<Option<Arc<RawPayloadStore>>>>,
    postgres_coordination: bool,
}

const POSTGRES_MAINTENANCE_LOCK_KEY: i64 = 0x7066_6d61_696e_746e;

pub(super) fn spawn(
    config: &WorkerConfig,
    pool: PgPool,
    retention: Arc<RwLock<UsageRetentionSettings>>,
    raw_store: Arc<RwLock<Option<Arc<RawPayloadStore>>>>,
    control: RuntimeControl,
) -> JoinHandle<()> {
    let valkey_url = config.valkey_url.trim().to_string();
    tokio::spawn(async move {
        let dependencies = Arc::new(MaintenanceDependencies {
            pool,
            retention,
            raw_store,
            postgres_coordination: valkey_url.is_empty(),
        });

        if !wait_for_initial_delay(&control).await {
            return;
        }

        if valkey_url.is_empty() {
            if let Err(error) = run_once(&dependencies).await {
                warn!(error = %error, "initial partition maintenance failed");
            }
            run_local_scheduler(dependencies, control).await;
            return;
        }

        let Some(store) = connect_coordinated_store(&valkey_url).await else {
            return;
        };

        if let Err(error) = run_once(&dependencies).await {
            warn!(error = %error, "initial partition maintenance failed");
        }

        let scheduler = coordinated_scheduler(store);
        if let Err(error) = run_coordinated_scheduler(
            scheduler,
            maintenance_job(dependencies.clone()),
            valkey_url.clone(),
            control.clone(),
        )
        .await
        {
            warn!(
                error = %error,
                capability = "maintenance_coordination",
                "coordinated maintenance scheduler stopped; local scheduling is not safe"
            );
        }
    })
}

/// Delay the first maintenance run by a random 5-15 minute window. The
/// steady-state 15-minute tick starts after this jittered delay.
async fn wait_for_initial_delay(control: &RuntimeControl) -> bool {
    let jitter = initial_delay_jitter();
    tokio::select! {
        _ = tokio::time::sleep(jitter) => true,
        _ = control.wait_for_shutdown() => false,
    }
}

fn initial_delay_jitter() -> Duration {
    let min_secs = MAINTENANCE_INITIAL_DELAY_MIN.as_secs();
    let max_secs = MAINTENANCE_INITIAL_DELAY_MAX.as_secs();
    Duration::from_secs(rand::rng().random_range(min_secs..=max_secs))
}

async fn run_local_scheduler(dependencies: Arc<MaintenanceDependencies>, control: RuntimeControl) {
    let scheduler = Scheduler::new(SchedulerConfig::default(), InMemoryStateStore::new());
    if let Err(error) = run_scheduler(scheduler, maintenance_job(dependencies), control).await {
        warn!(error = %error, "local maintenance scheduler stopped");
    }
}

fn maintenance_job(dependencies: Arc<MaintenanceDependencies>) -> Job<MaintenanceDependencies> {
    Job::new(
        MAINTENANCE_SCHEDULER_JOB_ID,
        Schedule::Interval(MAINTENANCE_INTERVAL),
        dependencies,
        Task::from_async(|context: TaskContext<MaintenanceDependencies>| async move {
            run_once(&context.deps)
                .await
                .map_err(|error| error.to_string())
        }),
    )
}

async fn run_once(dependencies: &MaintenanceDependencies) -> anyhow::Result<()> {
    let mut postgres_lease = if dependencies.postgres_coordination {
        match try_postgres_advisory_lease(&dependencies.pool, POSTGRES_MAINTENANCE_LOCK_KEY).await?
        {
            Some(lease) => Some(lease),
            None => return Ok(()),
        }
    } else {
        None
    };
    let retention = dependencies.retention.read().await.clone().normalized();
    let horizons = db::PartitionHorizons {
        metadata_retention_days: i64::from(retention.metadata_retention_days),
        content_retention_days: i64::from(retention.content_retention_days),
    };
    match db::run_partition_maintenance(&dependencies.pool, horizons).await {
        Ok(Some(report)) => info!(
            partitions_created = report.partitions_created,
            partitions_dropped = report.partitions_dropped,
            metadata_retention_days = retention.metadata_retention_days,
            content_retention_days = retention.content_retention_days,
            "partition maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => warn!(error = %error, "partition maintenance failed"),
    }
    // Raw payloads live in the managed object store; expired per-event
    // objects must be removed from the store before their metadata
    // partitions disappear.
    let raw_store = dependencies.raw_store.read().await.clone();
    match db::run_raw_payload_maintenance_with_store(
        &dependencies.pool,
        i64::from(retention.raw_retention_days),
        raw_store.as_deref(),
    )
    .await
    {
        Ok(Some(report)) => info!(
            raw_rows_deleted = report.raw_rows_deleted,
            retention_days = retention.raw_retention_days,
            "raw payload maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => {
            warn!(error = %error, "raw payload maintenance failed");
        }
    }
    // Plain tables outside the partition lifecycle still need bounded
    // cleanups: partition drops can orphan leases and redaction sessions.
    match db::cleanup_orphan_request_record_leases(&dependencies.pool).await {
        Ok(deleted) => info!(
            orphan_leases_deleted = deleted,
            "request lease cleanup completed"
        ),
        Err(error) => warn!(error = %error, "request lease cleanup failed"),
    }
    match db::cleanup_stale_conversation_redaction_sessions(&dependencies.pool).await {
        Ok(deleted) => info!(
            redaction_sessions_deleted = deleted,
            "redaction session cleanup completed"
        ),
        Err(error) => warn!(error = %error, "redaction session cleanup failed"),
    }
    match db::run_approval_retention_maintenance(
        &dependencies.pool,
        i64::from(retention.approval_retention_days),
    )
    .await
    {
        Ok(Some(deleted)) => info!(
            approval_rows_deleted = deleted,
            approval_retention_days = retention.approval_retention_days,
            "approval retention maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => warn!(error = %error, "approval retention maintenance failed"),
    }
    if let Some(lease) = postgres_lease.take() {
        lease.release().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    #[test]
    fn initial_maintenance_delay_is_jittered_within_five_to_fifteen_minutes() {
        for _ in 0..64 {
            let delay = initial_delay_jitter();
            assert!(delay >= MAINTENANCE_INITIAL_DELAY_MIN);
            assert!(delay <= MAINTENANCE_INITIAL_DELAY_MAX);
        }
        assert_eq!(MAINTENANCE_INITIAL_DELAY_MIN, Duration::from_secs(5 * 60));
        assert_eq!(MAINTENANCE_INITIAL_DELAY_MAX, Duration::from_secs(15 * 60));
        assert_eq!(MAINTENANCE_INTERVAL, Duration::from_secs(15 * 60));
    }

    #[tokio::test]
    async fn initial_maintenance_delay_returns_early_during_shutdown() {
        let control = RuntimeControl::new();
        control.begin_shutdown();
        assert!(!wait_for_initial_delay(&control).await);
    }

    #[tokio::test]
    async fn local_scheduler_stops_when_runtime_control_shuts_down() {
        let scheduler = Scheduler::new(SchedulerConfig::default(), InMemoryStateStore::new());
        let control = RuntimeControl::new();
        let executions = Arc::new(AtomicUsize::new(0));
        let execution_started = Arc::new(Notify::new());
        let task_executions = executions.clone();
        let task_execution_started = execution_started.clone();
        let job = Job::new(
            "partition-maintenance-test",
            Schedule::Interval(Duration::from_millis(1)),
            Arc::<()>::new(()),
            Task::from_async(move |_: TaskContext<()>| {
                let task_executions = task_executions.clone();
                let task_execution_started = task_execution_started.clone();
                async move {
                    task_executions.fetch_add(1, Ordering::Relaxed);
                    task_execution_started.notify_one();
                    Ok(())
                }
            }),
        );

        let task = tokio::spawn(run_scheduler(scheduler, job, control.clone()));
        tokio::time::timeout(Duration::from_secs(1), execution_started.notified())
            .await
            .expect("local scheduler should execute the test job");
        control.begin_shutdown();
        let result = task.await.expect("scheduler task should not panic");
        assert!(result.is_ok());
        assert!(executions.load(Ordering::Relaxed) > 0);
    }
}
