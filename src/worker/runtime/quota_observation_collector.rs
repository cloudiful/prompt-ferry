//! Coordinated periodic sampler for generic per-key quota observations
//! (issue #863 P2).
//!
//! The sampler reuses the coordinated scheduler plumbing in
//! [`super::background_job`]: one 60-second tick, a unique job name so
//! multi-worker runs stay single-flight, and shutdown through the shared
//! [`RuntimeControl`]. Due-ness comes from the last actually persisted
//! observation per key — never the tick clock — so cached results are not
//! duplicated: an active endpoint (request activity in the last 30 minutes)
//! refreshes every 5 minutes, an idle one every 30 minutes. Due endpoints
//! force a fresh fetch with the periodic source, which also retries a
//! previously failed persist. Expired observations prune hourly in bounded
//! batches, inside the same coordination.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tracing::warn;
use uuid::Uuid;

use crate::{
    config::WorkerConfig,
    db::{quota_key_observations, quota_snapshots},
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};
use scheduler::{Job, Schedule, Task, TaskContext};

use super::{
    background_job::{connect_coordinated_store, coordinated_scheduler, run_coordinated_scheduler},
    lifecycle::RuntimeControl,
};

const COLLECTOR_TICK: Duration = Duration::from_secs(60);
const ACTIVE_REFRESH_AFTER: ChronoDuration = ChronoDuration::minutes(5);
const IDLE_REFRESH_AFTER: ChronoDuration = ChronoDuration::minutes(30);
const ACTIVITY_LOOKBACK: ChronoDuration = ChronoDuration::minutes(30);
const PRUNE_INTERVAL: ChronoDuration = ChronoDuration::hours(1);
const MAX_CONCURRENT_REFRESHES: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_needs_a_first_observation_or_an_aged_one() {
        let now = Utc::now();
        assert!(!endpoint_due(&[], true, now));
        assert!(endpoint_due(&[None], false, now));
        assert!(endpoint_due(
            &[Some(now - ChronoDuration::minutes(6))],
            true,
            now
        ));
        assert!(!endpoint_due(
            &[Some(now - ChronoDuration::minutes(4))],
            true,
            now
        ));
        assert!(!endpoint_due(
            &[Some(now - ChronoDuration::minutes(29))],
            false,
            now
        ));
        assert!(endpoint_due(
            &[Some(now - ChronoDuration::minutes(31))],
            false,
            now
        ));
        // One due key refreshes the endpoint even when siblings are fresh.
        assert!(endpoint_due(
            &[Some(now - ChronoDuration::minutes(1)), None],
            true,
            now
        ));
    }

    #[test]
    fn prune_runs_once_an_hour() {
        let now = Utc::now();
        assert!(prune_due(None, now));
        assert!(!prune_due(Some(now - ChronoDuration::minutes(59)), now));
        assert!(prune_due(Some(now - ChronoDuration::minutes(61)), now));
    }
}

pub(super) fn spawn(
    config: &WorkerConfig,
    pool: PgPool,
    cache: TokenPlanQuotaCache,
    control: RuntimeControl,
) -> JoinHandle<()> {
    let valkey_url = config.valkey_url.trim().to_string();
    tokio::spawn(async move {
        if !periodic_coordination_configured(&valkey_url) {
            warn!(
                capability = "quota_observation_collection",
                "periodic quota observation collection disabled because Valkey coordination is not configured"
            );
        } else if let Some(store) = connect_coordinated_store(&valkey_url).await {
            let scheduler = coordinated_scheduler(store);
            if let Err(error) = run_coordinated_scheduler(
                scheduler,
                collector_job(pool, cache, control.clone()),
                valkey_url,
                control.clone(),
            )
            .await
            {
                warn!(
                    error = %error,
                    capability = "quota_observation_collection",
                    "coordinated quota observation collector stopped"
                );
                control.wait_for_shutdown().await;
            }
        } else {
            control.wait_for_shutdown().await;
        }
        if !control.is_shutting_down() {
            control.wait_for_shutdown().await;
        }
    })
}

fn periodic_coordination_configured(valkey_url: &str) -> bool {
    !valkey_url.trim().is_empty()
}

/// Due when any key was never observed or its last actual observation is at
/// least the cadence old. Pure for unit tests; the pass below feeds it DB
/// timestamps.
fn endpoint_due(
    latest_per_key: &[Option<DateTime<Utc>>],
    active: bool,
    now: DateTime<Utc>,
) -> bool {
    let interval = if active {
        ACTIVE_REFRESH_AFTER
    } else {
        IDLE_REFRESH_AFTER
    };
    latest_per_key.iter().any(|latest| {
        latest.is_none_or(|observed_at| now.signed_duration_since(observed_at) >= interval)
    })
}

fn prune_due(last_prune: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last_prune.is_none_or(|at| now.signed_duration_since(at) >= PRUNE_INTERVAL)
}

struct CollectorRuntime {
    pool: PgPool,
    cache: TokenPlanQuotaCache,
    control: RuntimeControl,
    last_prune: Arc<tokio::sync::Mutex<Option<DateTime<Utc>>>>,
}

async fn run_periodic_pass_until_shutdown(
    control: &RuntimeControl,
    pass: impl std::future::Future<Output = ()>,
) {
    tokio::select! {
        _ = control.wait_for_shutdown() => {},
        _ = pass => {},
    }
}

fn collector_job(
    pool: PgPool,
    cache: TokenPlanQuotaCache,
    control: RuntimeControl,
) -> Job<CollectorRuntime> {
    Job::new(
        "quota-observation-collector",
        Schedule::Interval(COLLECTOR_TICK),
        CollectorRuntime {
            pool,
            cache,
            control,
            last_prune: Arc::new(tokio::sync::Mutex::new(None)),
        },
        Task::from_async(|context: TaskContext<CollectorRuntime>| async move {
            run_periodic_pass_until_shutdown(
                &context.deps.control,
                run_periodic_pass(
                    &context.deps.pool,
                    &context.deps.cache,
                    &context.deps.last_prune,
                    Utc::now(),
                ),
            )
            .await;
            Ok::<(), String>(())
        }),
    )
}

async fn run_periodic_pass(
    pool: &PgPool,
    cache: &TokenPlanQuotaCache,
    last_prune: &tokio::sync::Mutex<Option<DateTime<Utc>>>,
    now: DateTime<Utc>,
) {
    let eligible = match quota_key_observations::eligible_endpoints(pool).await {
        Ok(eligible) => eligible,
        Err(_) => {
            warn!("quota observation collector could not list eligible endpoints");
            return;
        }
    };
    if eligible.is_empty() {
        return;
    }
    let mut by_endpoint: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in &eligible {
        by_endpoint
            .entry(row.endpoint_id)
            .or_default()
            .push(row.key_id);
    }
    let endpoint_ids: Vec<Uuid> = by_endpoint.keys().copied().collect();
    let active: HashSet<Uuid> = match quota_snapshots::endpoint_activity(
        pool,
        &endpoint_ids,
        now - ACTIVITY_LOOKBACK,
    )
    .await
    {
        Ok(activity) => activity.into_iter().map(|row| row.endpoint_id).collect(),
        Err(_) => {
            warn!("quota observation collector could not read endpoint activity");
            HashSet::new()
        }
    };

    stream::iter(by_endpoint)
        .for_each_concurrent(MAX_CONCURRENT_REFRESHES, |(endpoint_id, key_ids)| {
            let is_active = active.contains(&endpoint_id);
            async move {
                refresh_endpoint_if_due(pool, cache, endpoint_id, &key_ids, is_active, now).await;
            }
        })
        .await;

    if prune_due(*last_prune.lock().await, now) {
        match quota_key_observations::delete_expired_observations(
            pool,
            now,
            quota_key_observations::OBSERVATION_PRUNE_BATCH_SIZE,
        )
        .await
        {
            Ok(_) => {}
            Err(_) => {
                warn!("quota observation collector prune failed");
            }
        }
        *last_prune.lock().await = Some(now);
    }
}

async fn refresh_endpoint_if_due(
    pool: &PgPool,
    cache: &TokenPlanQuotaCache,
    endpoint_id: Uuid,
    key_ids: &[Uuid],
    active: bool,
    now: DateTime<Utc>,
) {
    let mut latest_per_key = Vec::with_capacity(key_ids.len());
    for key_id in key_ids {
        match quota_key_observations::latest_observation(pool, endpoint_id, *key_id).await {
            Ok(observation) => latest_per_key.push(observation.map(|row| row.observed_at)),
            Err(_) => {
                warn!(
                    endpoint_id = %endpoint_id,
                    "quota observation collector could not read the last observation"
                );
                return;
            }
        }
    }
    if !endpoint_due(&latest_per_key, active, now) {
        return;
    }
    if let Err(_) = cache.refresh_periodic(pool, endpoint_id).await {
        warn!(
            endpoint_id = %endpoint_id,
            "quota observation collector refresh failed"
        );
    }
}
