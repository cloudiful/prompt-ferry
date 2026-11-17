//! Worker discovery registry shared through the configured PostgreSQL database.
//!
//! A worker registers its stable configured identity plus the per-process
//! incarnation generated at startup. Heartbeats and exits are fenced by that
//! incarnation, so a superseded process can never overwrite the row of the
//! process that replaced it. Online state is derived from database time, never
//! from a stored boolean or the host clock.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::entities::postgres::workers;
use crate::db::seaorm::{native_error, postgres_connection};

/// How often a registered worker refreshes its heartbeat.
pub const WORKER_HEARTBEAT_SECONDS: u64 = 15;
/// A worker without a heartbeat inside this window is reported offline.
pub const WORKER_OFFLINE_AFTER_SECONDS: i64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStatus {
    Online,
    Offline,
    Stopped,
}

impl WorkerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCoordinationMode {
    /// No shared Valkey: the documented single-worker bootstrap mode.
    Bootstrap,
    /// Shared Valkey coordination at the applied configuration revision.
    Shared,
}

impl WorkerCoordinationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Shared => "shared",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        match value {
            "bootstrap" => Some(Self::Bootstrap),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// The coordination state a process starts with and the registry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinationSnapshot {
    pub mode: WorkerCoordinationMode,
    pub revision: Option<i64>,
}

impl CoordinationSnapshot {
    pub fn bootstrap(revision: Option<i64>) -> Self {
        Self {
            mode: WorkerCoordinationMode::Bootstrap,
            revision,
        }
    }

    pub fn shared(revision: Option<i64>) -> Self {
        Self {
            mode: WorkerCoordinationMode::Shared,
            revision,
        }
    }
}

/// Live registry members observed by one registration decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveWorkerCompatibility {
    pub identity_collisions: i64,
    pub live_workers: i64,
    pub live_incompatible: i64,
}

#[derive(Debug, Clone)]
pub struct WorkerRecord {
    pub worker_id: String,
    pub role: String,
    pub version: String,
    pub incarnation: Uuid,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub config_applied_revision: Option<i64>,
    pub config_restart_required: bool,
    pub coordination_mode: WorkerCoordinationMode,
    pub status: WorkerStatus,
}

impl WorkerRecord {
    fn from_model(model: workers::Model, db_now: DateTime<Utc>) -> Self {
        let status = if model.stopped_at.is_some() {
            WorkerStatus::Stopped
        } else if is_worker_live(model.stopped_at, model.last_heartbeat_at, db_now) {
            WorkerStatus::Online
        } else {
            WorkerStatus::Offline
        };
        Self {
            worker_id: model.worker_id,
            role: model.role,
            version: model.version,
            incarnation: model.incarnation,
            started_at: model.started_at,
            last_heartbeat_at: model.last_heartbeat_at,
            stopped_at: model.stopped_at,
            config_applied_revision: model.config_applied_revision,
            config_restart_required: model.config_restart_required,
            coordination_mode: WorkerCoordinationMode::from_db(&model.coordination_mode)
                .unwrap_or(WorkerCoordinationMode::Bootstrap),
            status,
        }
    }

    /// A live worker whose applied shared revision differs from the desired
    /// revision is the only state that can masquerade as coherent.
    fn with_desired_revision(mut self, desired: Option<i64>) -> Self {
        self.config_restart_required = self.status == WorkerStatus::Online
            && desired.is_some_and(|desired| self.config_applied_revision != Some(desired));
        self
    }
}

/// Advisory lock that serializes worker registry registrations.
const WORKER_REGISTRY_LOCK: i64 = 8_675_309_002;

/// Result of a registry registration attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// This incarnation owns the registry row.
    Registered,
    /// Another live incarnation already owns this identity.
    IdentityCollision,
    /// The degraded single-worker mode already has a live worker.
    WorkerLimitReached,
    /// The candidate mode or applied revision is not the deployment's desired
    /// coordination state, or live members use a different one.
    CoordinationIncompatible,
}

/// True while a worker row belongs to a process that is still running.
///
/// Online state is derived from database time: a stopped row is never live
/// and a row without a recent heartbeat has expired.
pub fn is_worker_live(
    stopped_at: Option<DateTime<Utc>>,
    last_heartbeat_at: DateTime<Utc>,
    db_now: DateTime<Utc>,
) -> bool {
    stopped_at.is_none()
        && last_heartbeat_at >= db_now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS)
}

/// Decide the outcome of a registration from the live rows it observed.
///
/// A live row of another incarnation that owns the identity is a collision
/// and is never replaced. The candidate must match the deployment's desired
/// coordination state: a stale startup configuration is rejected even when no
/// live member exists. In the degraded bootstrap mode any other live worker
/// blocks the registration; in shared mode only a live member that is not
/// shared or that applied a different revision blocks it.
pub fn registration_decision(
    candidate: CoordinationSnapshot,
    desired: CoordinationSnapshot,
    live: LiveWorkerCompatibility,
) -> RegisterOutcome {
    if live.identity_collisions > 0 {
        return RegisterOutcome::IdentityCollision;
    }
    if candidate != desired {
        return RegisterOutcome::CoordinationIncompatible;
    }
    match candidate.mode {
        WorkerCoordinationMode::Bootstrap => {
            if live.live_workers > 0 {
                RegisterOutcome::WorkerLimitReached
            } else {
                RegisterOutcome::Registered
            }
        }
        WorkerCoordinationMode::Shared => {
            if live.live_incompatible > 0 {
                RegisterOutcome::CoordinationIncompatible
            } else {
                RegisterOutcome::Registered
            }
        }
    }
}

/// Register this process without ever taking an identity from a live worker.
///
/// The decision is taken under a database lock, so two processes that start
/// at the same time cannot both believe they own the identity. A live row of
/// another incarnation is never replaced: the caller receives a collision
/// instead.
pub async fn register_worker(
    pool: &PgPool,
    worker_id: &str,
    role: &str,
    version: &str,
    incarnation: Uuid,
    candidate: CoordinationSnapshot,
    config_restart_required: bool,
) -> Result<RegisterOutcome> {
    let mut transaction = pool.begin().await?;
    // Take the settings lock before the registry lock: a shared-Valkey write
    // can then never commit between the desired-state read and this
    // registration. Writers take the settings lock first and no other
    // coordination lock while holding it, so this order cannot deadlock.
    super::shared_valkey::lock_shared_valkey_setting(&mut transaction).await?;
    sqlx::query_file!(
        "src/sql/workers/lock_worker_registry.sql",
        WORKER_REGISTRY_LOCK
    )
    .execute(&mut *transaction)
    .await?;
    let desired_row = sqlx::query_file!(
        "src/sql/workers/desired_coordination.sql",
        super::shared_valkey::SHARED_VALKEY_SETTINGS_KEY
    )
    .fetch_optional(&mut *transaction)
    .await?;
    let desired = match desired_row {
        Some(row) => {
            let mode = if row.configured.unwrap_or(false) {
                WorkerCoordinationMode::Shared
            } else {
                WorkerCoordinationMode::Bootstrap
            };
            CoordinationSnapshot {
                mode,
                revision: Some(row.revision.unwrap_or(0)),
            }
        }
        None => CoordinationSnapshot::bootstrap(None),
    };
    let identity_collisions: i64 = sqlx::query_file_scalar!(
        "src/sql/workers/count_live_identity.sql",
        worker_id,
        WORKER_OFFLINE_AFTER_SECONDS,
        incarnation
    )
    .fetch_one(&mut *transaction)
    .await?
    .unwrap_or(0);
    let live_row = sqlx::query_file!(
        "src/sql/workers/read_live_workers.sql",
        WORKER_OFFLINE_AFTER_SECONDS,
        incarnation,
        candidate.revision
    )
    .fetch_one(&mut *transaction)
    .await?;
    let live = LiveWorkerCompatibility {
        identity_collisions,
        live_workers: live_row.live_count.unwrap_or(0),
        live_incompatible: live_row.incompatible_count.unwrap_or(0),
    };
    let outcome = registration_decision(candidate, desired, live);
    if outcome != RegisterOutcome::Registered {
        transaction.rollback().await?;
        return Ok(outcome);
    }
    sqlx::query_file!(
        "src/sql/workers/register_worker.sql",
        worker_id,
        role,
        version,
        incarnation,
        candidate.revision,
        config_restart_required,
        candidate.mode.as_str(),
        WORKER_OFFLINE_AFTER_SECONDS
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(RegisterOutcome::Registered)
}

/// Count the live worker rows in the registry, excluding one incarnation.
pub async fn count_live_workers(pool: &PgPool, excluding: Option<Uuid>) -> Result<i64> {
    let db_now: chrono::DateTime<chrono::Utc> =
        sqlx::query_file_scalar!("src/sql/workers/database_now.sql")
            .fetch_one(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("database time query returned no row"))?;
    let db = postgres_connection(pool);
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
    let offline_before = db_now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS);
    let mut query = workers::Entity::find()
        .filter(workers::Column::StoppedAt.is_null())
        .filter(workers::Column::LastHeartbeatAt.gte(offline_before));
    if let Some(incarnation) = excluding {
        query = query.filter(workers::Column::Incarnation.ne(incarnation));
    }
    let count = query.count(&db).await.map_err(native_error)?;
    Ok(count as i64)
}

/// Refresh the heartbeat of this incarnation. Returns false once another
/// incarnation owns the identity, so the caller can stop pretending to be live.
pub async fn heartbeat_worker(
    pool: &PgPool,
    worker_id: &str,
    incarnation: Uuid,
    config_applied_revision: Option<i64>,
    config_restart_required: bool,
    coordination_mode: WorkerCoordinationMode,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/workers/heartbeat_worker.sql",
        worker_id,
        incarnation,
        config_applied_revision,
        config_restart_required,
        coordination_mode.as_str()
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark this incarnation stopped. A superseded incarnation changes nothing.
pub async fn stop_worker(pool: &PgPool, worker_id: &str, incarnation: Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/workers/stop_worker.sql", worker_id, incarnation)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Read one page of the registry plus totals. `online_count` covers the whole
/// registry, not only the returned page.
pub async fn list_workers(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<(i64, i64, Vec<WorkerRecord>)> {
    let db = postgres_connection(pool);
    let db_now: DateTime<Utc> = sqlx::query_file_scalar!("src/sql/workers/database_now.sql")
        .fetch_one(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("database time query returned no row"))?;
    let total = workers::Entity::find()
        .count(&db)
        .await
        .map_err(native_error)? as i64;
    let offline_before = db_now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS);
    let online_count = workers::Entity::find()
        .filter(workers::Column::StoppedAt.is_null())
        .filter(workers::Column::LastHeartbeatAt.gte(offline_before))
        .count(&db)
        .await
        .map_err(native_error)? as i64;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let desired = super::desired_shared_valkey_revision(pool).await?;
    let records = workers::Entity::find()
        .order_by_desc(workers::Column::LastHeartbeatAt)
        .order_by_asc(workers::Column::WorkerId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((
        total,
        online_count,
        records
            .into_iter()
            .map(|model| WorkerRecord::from_model(model, db_now).with_desired_revision(desired))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(
        last_heartbeat_at: DateTime<Utc>,
        stopped_at: Option<DateTime<Utc>>,
    ) -> workers::Model {
        workers::Model {
            worker_id: "worker-a".to_string(),
            role: "worker".to_string(),
            version: "1.0.0".to_string(),
            incarnation: Uuid::nil(),
            started_at: last_heartbeat_at,
            last_heartbeat_at,
            stopped_at,
            config_applied_revision: Some(7),
            config_restart_required: false,
            coordination_mode: "shared".to_string(),
        }
    }

    #[test]
    fn a_recent_heartbeat_is_online_and_an_expired_one_is_offline() {
        let now = Utc::now();
        let online = WorkerRecord::from_model(
            model(
                now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS - 1),
                None,
            ),
            now,
        );
        assert_eq!(online.status, WorkerStatus::Online);

        let offline = WorkerRecord::from_model(
            model(
                now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS + 1),
                None,
            ),
            now,
        );
        assert_eq!(offline.status, WorkerStatus::Offline);
    }

    #[test]
    fn a_stopped_row_is_stopped_even_with_a_recent_heartbeat() {
        let now = Utc::now();
        let stopped = WorkerRecord::from_model(model(now, Some(now)), now);
        assert_eq!(stopped.status, WorkerStatus::Stopped);
    }

    #[test]
    fn a_live_row_of_another_incarnation_is_a_collision_and_is_never_replaced() {
        let candidate = CoordinationSnapshot::shared(Some(7));
        let live = LiveWorkerCompatibility {
            identity_collisions: 1,
            ..LiveWorkerCompatibility::default()
        };
        assert_eq!(
            registration_decision(candidate, candidate, live),
            RegisterOutcome::IdentityCollision
        );
        assert_eq!(
            registration_decision(
                CoordinationSnapshot::bootstrap(None),
                CoordinationSnapshot::bootstrap(None),
                live
            ),
            RegisterOutcome::IdentityCollision
        );
    }

    #[test]
    fn the_degraded_bootstrap_mode_admits_only_one_live_worker() {
        let bootstrap = CoordinationSnapshot::bootstrap(None);
        assert_eq!(
            registration_decision(bootstrap, bootstrap, LiveWorkerCompatibility::default()),
            RegisterOutcome::Registered
        );
        assert_eq!(
            registration_decision(
                bootstrap,
                bootstrap,
                LiveWorkerCompatibility {
                    live_workers: 1,
                    ..LiveWorkerCompatibility::default()
                }
            ),
            RegisterOutcome::WorkerLimitReached
        );
    }

    #[test]
    fn shared_mode_admits_distinct_workers_on_the_same_applied_revision() {
        let shared = CoordinationSnapshot::shared(Some(7));
        assert_eq!(
            registration_decision(shared, shared, LiveWorkerCompatibility::default()),
            RegisterOutcome::Registered
        );
        // Live rows at the same mode and revision are compatible: only the
        // incompatible count blocks the admission, not the total live count.
        assert_eq!(
            registration_decision(
                shared,
                shared,
                LiveWorkerCompatibility {
                    live_workers: 4,
                    live_incompatible: 0,
                    identity_collisions: 0,
                }
            ),
            RegisterOutcome::Registered
        );
    }

    #[test]
    fn a_shared_worker_is_rejected_while_a_bootstrap_worker_is_still_live() {
        let shared = CoordinationSnapshot::shared(Some(7));
        assert_eq!(
            registration_decision(
                shared,
                shared,
                LiveWorkerCompatibility {
                    live_workers: 1,
                    live_incompatible: 1,
                    identity_collisions: 0,
                }
            ),
            RegisterOutcome::CoordinationIncompatible,
            "bootstrap -> shared needs the old worker stopped first"
        );
    }

    #[test]
    fn a_changed_shared_revision_rejects_mixed_live_members_and_stale_starts() {
        let shared_new = CoordinationSnapshot::shared(Some(8));
        assert_eq!(
            registration_decision(
                shared_new,
                shared_new,
                LiveWorkerCompatibility {
                    live_workers: 2,
                    live_incompatible: 2,
                    identity_collisions: 0,
                }
            ),
            RegisterOutcome::CoordinationIncompatible,
            "existing workers at the old revision must stop before the restart"
        );
        assert_eq!(
            registration_decision(
                CoordinationSnapshot::shared(Some(7)),
                shared_new,
                LiveWorkerCompatibility::default()
            ),
            RegisterOutcome::CoordinationIncompatible,
            "a startup that resolved the old desired revision is stale"
        );
    }

    #[test]
    fn shared_to_disabled_requires_every_live_member_to_stop_first() {
        let disabled = CoordinationSnapshot::bootstrap(Some(9));
        assert_eq!(
            registration_decision(
                disabled,
                disabled,
                LiveWorkerCompatibility {
                    live_workers: 1,
                    live_incompatible: 0,
                    identity_collisions: 0,
                }
            ),
            RegisterOutcome::WorkerLimitReached,
            "the one-worker bootstrap mode must not start beside a live worker"
        );
        assert_eq!(
            registration_decision(disabled, disabled, LiveWorkerCompatibility::default()),
            RegisterOutcome::Registered
        );
    }

    #[test]
    fn a_bootstrap_start_is_stale_when_the_desired_settings_record_a_revision() {
        assert_eq!(
            registration_decision(
                CoordinationSnapshot::bootstrap(None),
                CoordinationSnapshot::bootstrap(Some(9)),
                LiveWorkerCompatibility::default()
            ),
            RegisterOutcome::CoordinationIncompatible
        );
        assert_eq!(
            registration_decision(
                CoordinationSnapshot::bootstrap(None),
                CoordinationSnapshot::shared(Some(9)),
                LiveWorkerCompatibility::default()
            ),
            RegisterOutcome::CoordinationIncompatible
        );
    }

    #[test]
    fn liveness_follows_the_database_clock_and_the_stopped_flag() {
        let now = Utc::now();
        assert!(is_worker_live(
            None,
            now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS - 1),
            now
        ));
        assert!(!is_worker_live(
            None,
            now - Duration::seconds(WORKER_OFFLINE_AFTER_SECONDS + 1),
            now
        ));
        assert!(!is_worker_live(Some(now), now, now));
    }

    #[test]
    fn only_a_live_worker_on_an_older_applied_revision_requires_restart() {
        let now = Utc::now();
        let online = WorkerRecord::from_model(model(now, None), now);
        let restart = online.clone().with_desired_revision(Some(8));
        assert_eq!(restart.status, WorkerStatus::Online);
        assert!(restart.config_restart_required);
        assert!(
            !online
                .clone()
                .with_desired_revision(Some(7))
                .config_restart_required
        );
        assert!(!online.with_desired_revision(None).config_restart_required);

        let stopped = WorkerRecord::from_model(model(now, Some(now)), now);
        assert!(
            !stopped
                .with_desired_revision(Some(8))
                .config_restart_required
        );
    }
}
