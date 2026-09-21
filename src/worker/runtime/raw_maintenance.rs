use super::background_job::{
    connect_coordinated_store, coordinated_scheduler, run_coordinated_scheduler, run_scheduler,
    try_postgres_advisory_lease,
};
use super::lifecycle::RuntimeControl;
use crate::raw_payload_store::RawPayloadStore;
use crate::worker_admin_types::UsageRetentionSettings;
use crate::{config::WorkerConfig, db};
use scheduler::{InMemoryStateStore, Job, Schedule, Scheduler, SchedulerConfig, Task, TaskContext};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{info, warn};

const RAW_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60 * 60);
const RAW_SCHEDULER_JOB_ID: &str = "prompt-ferry:raw-payload-maintenance";

#[derive(Clone)]
struct RawMaintenanceDependencies {
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
        let dependencies = Arc::new(RawMaintenanceDependencies {
            pool,
            retention,
            raw_store,
            postgres_coordination: valkey_url.is_empty(),
        });

        if valkey_url.is_empty() {
            if let Err(error) = run_once(&dependencies).await {
                warn!(error = %error, "initial raw payload maintenance failed");
            }
            run_local_scheduler(dependencies, control).await;
            return;
        }

        let Some(store) = connect_coordinated_store(&valkey_url).await else {
            return;
        };

        if let Err(error) = run_once(&dependencies).await {
            warn!(error = %error, "initial raw payload maintenance failed");
        }

        let scheduler = coordinated_scheduler(store);
        if let Err(error) = run_coordinated_scheduler(
            scheduler,
            raw_maintenance_job(dependencies.clone()),
            valkey_url.clone(),
            control.clone(),
        )
        .await
        {
            warn!(
                error = %error,
                capability = "maintenance_coordination",
                "coordinated raw maintenance scheduler stopped; local scheduling is not safe"
            );
        }
    })
}

async fn run_local_scheduler(
    dependencies: Arc<RawMaintenanceDependencies>,
    control: RuntimeControl,
) {
    let scheduler = Scheduler::new(SchedulerConfig::default(), InMemoryStateStore::new());
    if let Err(error) = run_scheduler(scheduler, raw_maintenance_job(dependencies), control).await {
        warn!(error = %error, "local raw maintenance scheduler stopped");
    }
}

fn raw_maintenance_job(
    dependencies: Arc<RawMaintenanceDependencies>,
) -> Job<RawMaintenanceDependencies> {
    Job::new(
        RAW_SCHEDULER_JOB_ID,
        Schedule::Interval(RAW_MAINTENANCE_INTERVAL),
        dependencies,
        Task::from_async(
            |context: TaskContext<RawMaintenanceDependencies>| async move {
                run_once(&context.deps)
                    .await
                    .map_err(|error| error.to_string())
            },
        ),
    )
}

async fn run_once(dependencies: &RawMaintenanceDependencies) -> anyhow::Result<()> {
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
    match db::run_usage_content_maintenance(
        &dependencies.pool,
        i64::from(retention.content_retention_days),
    )
    .await
    {
        Ok(Some(report)) => info!(
            expired_events = report.expired_events,
            deleted_block_refs = report.deleted_block_refs,
            deleted_artifacts = report.deleted_artifacts,
            deleted_snapshots = report.deleted_snapshots,
            cleared_tool_arguments = report.cleared_tool_arguments,
            deleted_redaction_sessions = report.deleted_redaction_sessions,
            orphan_prompt_blocks_deleted = report.orphan_prompt_blocks_deleted,
            content_retention_days = retention.content_retention_days,
            "usage content maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => warn!(error = %error, "usage content maintenance failed"),
    }
    match db::run_usage_metadata_maintenance(
        &dependencies.pool,
        i64::from(retention.metadata_retention_days),
    )
    .await
    {
        Ok(Some(report)) => info!(
            metadata_rows_deleted = report.deleted,
            protected_by_billing = report.protected_by_billing,
            metadata_retention_days = retention.metadata_retention_days,
            "usage metadata maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => warn!(error = %error, "usage metadata maintenance failed"),
    }
    // Raw payloads always live in the managed object store; maintenance only
    // prunes expired per-event object metadata and partitions.
    let raw_store = dependencies.raw_store.read().await.clone();
    match db::run_raw_payload_maintenance_with_store(
        &dependencies.pool,
        i64::from(retention.raw_retention_days),
        raw_store.as_deref(),
    )
    .await
    {
        Ok(Some(report)) => info!(
            partitions_created = report.partitions_created,
            raw_rows_deleted = report.raw_rows_deleted,
            partitions_dropped = report.partitions_dropped,
            retention_days = retention.raw_retention_days,
            "raw payload maintenance completed"
        ),
        Ok(None) => {}
        Err(error) => {
            warn!(error = %error, "raw payload maintenance failed");
        }
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

    #[tokio::test]
    async fn local_scheduler_stops_when_runtime_control_shuts_down() {
        let scheduler = Scheduler::new(SchedulerConfig::default(), InMemoryStateStore::new());
        let control = RuntimeControl::new();
        let executions = Arc::new(AtomicUsize::new(0));
        let execution_started = Arc::new(Notify::new());
        let task_executions = executions.clone();
        let task_execution_started = execution_started.clone();
        let job = Job::new(
            "raw-maintenance-test",
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
