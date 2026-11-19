use std::{cmp::Ordering, str::FromStr, time::Duration};

use anyhow::{Result, anyhow, bail};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use sqlx::{Connection, PgConnection, PgPool, Row};
use tokio::time::{Instant, sleep, timeout, timeout_at};
use uuid::Uuid;

use super::entities::postgres::upgrade_gate as entity;
use super::seaorm::PostgresNativeConnection;

mod schema;

pub const UPGRADE_CLAIM_KEY: i64 = 8_675_309_066;
pub const SESSION_BARRIER_KEY: i64 = 8_675_309_067;
pub const NODE_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RuntimeVersion(pub u64, pub u64, pub u64);

impl FromStr for RuntimeVersion {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|b| b.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
            })
        {
            return Err(anyhow!("unsupported runtime version"));
        }
        Ok(Self(
            parts[0].parse()?,
            parts[1].parse()?,
            parts[2].parse()?,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeIdentity {
    pub version: RuntimeVersion,
    pub protocol: u32,
}

impl RuntimeIdentity {
    pub fn current() -> Result<Self> {
        Ok(Self {
            version: env!("CARGO_PKG_VERSION").parse()?,
            protocol: NODE_PROTOCOL_VERSION,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateState {
    Ready,
    Draining,
    Migrating,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GateSnapshot {
    pub state: GateState,
    pub identity: RuntimeIdentity,
    pub generation: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionDecision {
    Join,
    Upgrade,
    Block,
    Reject,
}

pub fn decide_admission(identity: RuntimeIdentity, gate: GateSnapshot) -> AdmissionDecision {
    if gate.state != GateState::Ready {
        return AdmissionDecision::Block;
    }
    if identity.protocol < gate.identity.protocol || identity.version < gate.identity.version {
        return AdmissionDecision::Reject;
    }
    match identity.version.cmp(&gate.identity.version) {
        Ordering::Equal if identity.protocol == gate.identity.protocol => AdmissionDecision::Join,
        Ordering::Equal => AdmissionDecision::Reject,
        Ordering::Greater => AdmissionDecision::Upgrade,
        Ordering::Less => AdmissionDecision::Reject,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapDecision {
    Allow,
    RequireAcknowledgement,
    LiveWorkers,
    LiveRequestLeases,
}

pub fn decide_bootstrap(
    legacy_schema: bool,
    acknowledged: bool,
    live_workers: bool,
    live_request_leases: bool,
) -> BootstrapDecision {
    if live_workers {
        BootstrapDecision::LiveWorkers
    } else if live_request_leases {
        BootstrapDecision::LiveRequestLeases
    } else if legacy_schema && !acknowledged {
        BootstrapDecision::RequireAcknowledgement
    } else {
        BootstrapDecision::Allow
    }
}

impl std::fmt::Display for RuntimeVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

fn failure() -> anyhow::Error {
    anyhow!("node gate database operation failed")
}

fn snapshot(row: &entity::Model) -> Result<GateSnapshot> {
    let state = match row.state.as_str() {
        "ready" => GateState::Ready,
        "draining" => GateState::Draining,
        "migrating" => GateState::Migrating,
        "failed" => GateState::Failed,
        _ => bail!("invalid node gate state"),
    };
    let protocol = u32::try_from(row.protocol_version).map_err(|_| failure())?;
    if row.generation < 0
        || (state == GateState::Ready
            && (row.generation == 0
                || row.target_runtime_version.is_some()
                || row.target_protocol_version.is_some()
                || row.owner_id.is_some()))
    {
        bail!("invalid node gate metadata");
    }
    Ok(GateSnapshot {
        state,
        identity: RuntimeIdentity {
            version: row.runtime_version.parse()?,
            protocol,
        },
        generation: row.generation,
    })
}

fn expected_admission(expected: GateSnapshot, actual: GateSnapshot) -> bool {
    expected.state == GateState::Ready && expected.generation > 0 && actual == expected
}

fn lock_wait_remaining(now: Instant, deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(now);
    if remaining.is_zero() {
        bail!("node gate lock timed out");
    }
    Ok(remaining)
}

/// A dedicated connection used only for gate metadata. It is never an application pool.
pub struct GateControl {
    connection: PgConnection,
}

impl GateControl {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let connection = timeout(
            super::connection::POSTGRES_CONNECT_TIMEOUT,
            PgConnection::connect(database_url),
        )
        .await
        .map_err(|_| anyhow!("node gate connection timed out"))?
        .map_err(|_| failure())?;
        Ok(Self { connection })
    }

    pub async fn read(&mut self) -> Result<GateSnapshot> {
        let native = PostgresNativeConnection::new(&mut self.connection);
        let row = entity::Entity::find_by_id(1)
            .one(&native)
            .await
            .map_err(|_| failure())?
            .ok_or_else(failure)?;
        snapshot(&row)
    }
}

struct UpgradeSession {
    connection: PgConnection,
}

impl UpgradeSession {
    async fn acquire(database_url: &str, deadline: Instant) -> Result<Self> {
        let control = timeout_at(deadline, GateControl::connect(database_url))
            .await
            .map_err(|_| anyhow!("node gate claim timed out"))??;
        let mut session = Self {
            connection: control.connection,
        };
        session
            .wait_lock(include_str!("../sql/node_gate/try_claim.sql"), deadline)
            .await?;
        Ok(session)
    }

    async fn wait_lock(&mut self, statement: &'static str, deadline: Instant) -> Result<()> {
        loop {
            lock_wait_remaining(Instant::now(), deadline)?;
            let got: bool = timeout_at(
                deadline,
                sqlx::query_scalar(sqlx::AssertSqlSafe(statement)).fetch_one(&mut self.connection),
            )
            .await
            .map_err(|_| anyhow!("node gate lock timed out"))?
            .map_err(|_| failure())?;
            if got {
                return Ok(());
            }
            sleep(Duration::from_millis(100).min(lock_wait_remaining(Instant::now(), deadline)?))
                .await;
        }
    }

    async fn row(&mut self) -> Result<Option<entity::Model>> {
        let native = PostgresNativeConnection::new(&mut self.connection);
        entity::Entity::find_by_id(1)
            .one(&native)
            .await
            .map_err(|_| failure())
    }

    async fn transition(
        &mut self,
        owner: Uuid,
        identity: RuntimeIdentity,
        from: &str,
        to: &str,
        finish: bool,
    ) -> Result<()> {
        let protocol = i32::try_from(identity.protocol).map_err(|_| failure())?;
        let native = PostgresNativeConnection::new(&mut self.connection);
        let mut update = entity::Entity::update_many()
            .col_expr(entity::Column::State, Expr::value(to))
            .col_expr(entity::Column::UpdatedAt, Expr::current_timestamp())
            .filter(entity::Column::Id.eq(1))
            .filter(entity::Column::State.eq(from))
            .filter(entity::Column::OwnerId.eq(owner))
            .filter(entity::Column::TargetRuntimeVersion.eq(identity.version.to_string()))
            .filter(entity::Column::TargetProtocolVersion.eq(protocol));
        if finish {
            update = update
                .col_expr(
                    entity::Column::RuntimeVersion,
                    Expr::value(identity.version.to_string()),
                )
                .col_expr(entity::Column::ProtocolVersion, Expr::value(protocol))
                .col_expr(
                    entity::Column::Generation,
                    Expr::col(entity::Column::Generation).add(1),
                )
                .col_expr(
                    entity::Column::TargetRuntimeVersion,
                    Expr::value(None::<String>),
                )
                .col_expr(
                    entity::Column::TargetProtocolVersion,
                    Expr::value(None::<i32>),
                )
                .col_expr(entity::Column::OwnerId, Expr::value(None::<Uuid>));
        }
        if update
            .exec(&native)
            .await
            .map_err(|_| failure())?
            .rows_affected
            != 1
        {
            bail!("node gate ownership changed");
        }
        Ok(())
    }
}

async fn table_exists(connection: &mut PgConnection, table: &str) -> Result<bool> {
    let native = PostgresNativeConnection::new(connection);
    let query = Query::select()
        .expr_as(
            Expr::cust_with_values("to_regclass($1) IS NOT NULL", [format!("public.{table}")]),
            Alias::new("present"),
        )
        .to_owned();
    let row = native
        .query_one(&query)
        .await
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    row.try_get("", "present").map_err(|_| failure())
}

async fn has_rows(
    connection: &mut PgConnection,
    table: &str,
    predicate: sea_orm::sea_query::SimpleExpr,
) -> Result<bool> {
    let native = PostgresNativeConnection::new(connection);
    let query = Query::select()
        .expr(Expr::value(1))
        .from((Alias::new("public"), Alias::new(table)))
        .and_where(predicate)
        .limit(1)
        .to_owned();
    Ok(native
        .query_one(&query)
        .await
        .map_err(|_| failure())?
        .is_some())
}

async fn legacy_safety(connection: &mut PgConnection, acknowledged: bool) -> Result<()> {
    let workers = table_exists(connection, "workers").await?;
    let leases = table_exists(connection, "request_record_leases").await?;
    let records = table_exists(connection, "request_records").await?;
    // The old request-record lease columns predate the dedicated lease table.
    let native = PostgresNativeConnection::new(connection);
    let columns = Query::select().expr(Expr::value(1)).from((Alias::new("pg_catalog"), Alias::new("pg_attribute")))
        .and_where(Expr::cust("attrelid = to_regclass('public.request_records') AND attname = 'lease_expires_at' AND attnum > 0 AND NOT attisdropped"))
        .limit(1).to_owned();
    let old_lease_column = native
        .query_one(&columns)
        .await
        .map_err(|_| failure())?
        .is_some();
    let public_objects = Query::select()
        .expr(Expr::value(1))
        .from((Alias::new("pg_catalog"), Alias::new("pg_class")))
        .and_where(Expr::cust(
            "relnamespace = 'public'::regnamespace AND relkind IN ('r', 'p', 'v', 'm', 'f', 'S')",
        ))
        .limit(1)
        .to_owned();
    let has_public_objects = native
        .query_one(&public_objects)
        .await
        .map_err(|_| failure())?
        .is_some();
    let live_workers = if workers {
        has_rows(
            connection,
            "workers",
            Expr::cust_with_values(
                "stopped_at IS NULL AND last_heartbeat_at >= now() - ($1 * interval '1 second')",
                [super::WORKER_OFFLINE_AFTER_SECONDS],
            ),
        )
        .await?
    } else {
        false
    };
    let live_leases = if leases {
        has_rows(
            connection,
            "request_record_leases",
            Expr::cust("lease_expires_at > now() AND owner_worker_id IS NOT NULL"),
        )
        .await?
    } else {
        false
    } || if records && old_lease_column {
        has_rows(
            connection,
            "request_records",
            Expr::cust("lease_expires_at > now() AND owner_worker_id IS NOT NULL"),
        )
        .await?
    } else {
        false
    };
    match decide_bootstrap(workers, acknowledged, live_workers, live_leases) {
        BootstrapDecision::Allow if workers || !has_public_objects => Ok(()),
        BootstrapDecision::Allow => {
            bail!("unrecognized existing application schema without workers")
        }
        BootstrapDecision::RequireAcknowledgement => {
            bail!("legacy cutover requires operator acknowledgement that old nodes are stopped")
        }
        BootstrapDecision::LiveWorkers => bail!("legacy cutover refused: live registry workers"),
        BootstrapDecision::LiveRequestLeases => {
            bail!("legacy cutover refused: live request leases")
        }
    }
}

/// Fencing for an offline maintenance export.
///
/// The export takes the same upgrade claim and the exclusive session barrier a
/// database migration takes, on one owned control session, and holds both for
/// the whole export. A running or upgrading node therefore blocks the export
/// instead of competing with it, and no new runtime session can be admitted
/// while the export reads the source. The locks are advisory session locks: the
/// source database is neither written nor migrated.
pub struct ExportFence {
    session: UpgradeSession,
}

impl ExportFence {
    /// Claim the upgrade lock and the exclusive barrier, bounded by `wait`.
    pub async fn acquire(database_url: &str, wait: Duration) -> Result<Self> {
        let deadline = Instant::now().checked_add(wait).ok_or_else(failure)?;
        let mut session = UpgradeSession::acquire(database_url, deadline)
            .await
            .map_err(|error| anyhow!("export fencing unavailable: {error}"))?;
        session
            .wait_lock(include_str!("../sql/node_gate/try_exclusive.sql"), deadline)
            .await
            .map_err(|error| {
                anyhow!("export fencing unavailable: another session holds the exclusive barrier ({error})")
            })?;
        Ok(Self { session })
    }

    /// Close the control session, releasing both advisory locks.
    pub async fn release(self) {
        let _ = self.session.connection.close().await;
    }
}

pub struct NodeGate;

impl NodeGate {
    /// Prepare the schema on one dedicated claim and exclusive-barrier session.
    /// Legacy processes do not participate in the barrier and must be stopped by the operator.
    pub async fn prepare_database(
        database_url: &str,
        identity: RuntimeIdentity,
        owner: Uuid,
        stop_old_ack: bool,
        wait: Duration,
    ) -> Result<GateSnapshot> {
        let deadline = Instant::now().checked_add(wait).ok_or_else(failure)?;
        let mut session = UpgradeSession::acquire(database_url, deadline).await?;
        let exists = table_exists(&mut session.connection, "prompt_ferry_node_gate").await?;
        let existing =
            if exists {
                Some(session.row().await?.ok_or_else(|| {
                    anyhow!("node gate row missing; operator intervention required")
                })?)
            } else {
                None
            };
        if let Some(row) = existing {
            let current = snapshot(&row)?;
            match decide_admission(identity, current) {
                AdmissionDecision::Join => {
                    schema::guard(&mut session.connection, schema::GuardMode::Join, true).await?;
                    return Ok(current);
                }
                AdmissionDecision::Block => {
                    bail!("node gate is not ready; operator intervention required")
                }
                AdmissionDecision::Reject => bail!("node gate rejects runtime identity"),
                AdmissionDecision::Upgrade => {
                    schema::guard(&mut session.connection, schema::GuardMode::Upgrade, true)
                        .await?;
                }
            }
            let protocol = i32::try_from(identity.protocol).map_err(|_| failure())?;
            let native = PostgresNativeConnection::new(&mut session.connection);
            let result = entity::Entity::update_many()
                .col_expr(entity::Column::State, Expr::value("draining"))
                .col_expr(entity::Column::OwnerId, Expr::value(owner))
                .col_expr(
                    entity::Column::TargetRuntimeVersion,
                    Expr::value(identity.version.to_string()),
                )
                .col_expr(entity::Column::TargetProtocolVersion, Expr::value(protocol))
                .col_expr(entity::Column::UpdatedAt, Expr::current_timestamp())
                .filter(entity::Column::Id.eq(1))
                .filter(entity::Column::State.eq("ready"))
                .filter(entity::Column::RuntimeVersion.eq(row.runtime_version))
                .filter(entity::Column::ProtocolVersion.eq(row.protocol_version))
                .filter(entity::Column::Generation.eq(row.generation))
                .exec(&native)
                .await
                .map_err(|_| failure())?;
            if result.rows_affected != 1 {
                bail!("node gate changed during upgrade claim");
            }
            session
                .wait_lock(include_str!("../sql/node_gate/try_exclusive.sql"), deadline)
                .await?;
            session
                .transition(owner, identity, "draining", "migrating", false)
                .await?;
        } else {
            schema::guard(&mut session.connection, schema::GuardMode::Bootstrap, false).await?;
            session
                .wait_lock(include_str!("../sql/node_gate/try_exclusive.sql"), deadline)
                .await?;
            legacy_safety(&mut session.connection, stop_old_ack).await?;
            let mut tx = session.connection.begin().await.map_err(|_| failure())?;
            {
                let native = PostgresNativeConnection::new(&mut tx);
                let mut schema = entity::create_table_statement();
                schema.if_not_exists();
                native.execute(&schema).await.map_err(|_| failure())?;
                entity::ActiveModel {
                    id: Set(1),
                    state: Set("migrating".to_owned()),
                    runtime_version: Set(identity.version.to_string()),
                    protocol_version: Set(i32::try_from(identity.protocol).map_err(|_| failure())?),
                    generation: Set(0),
                    target_runtime_version: Set(Some(identity.version.to_string())),
                    target_protocol_version: Set(Some(
                        i32::try_from(identity.protocol).map_err(|_| failure())?,
                    )),
                    owner_id: Set(Some(owner)),
                    updated_at: sea_orm::ActiveValue::NotSet,
                }
                .insert(&native)
                .await
                .map_err(|_| failure())?;
            }
            tx.commit().await.map_err(|_| failure())?;
        }
        if super::connection::node_migrator()
            .run_direct(None, &mut session.connection, false)
            .await
            .is_err()
        {
            let _ = session
                .transition(owner, identity, "migrating", "failed", false)
                .await;
            bail!("node gate migration failed; operator intervention required");
        }
        session
            .transition(owner, identity, "migrating", "ready", true)
            .await?;
        let row = session.row().await?.ok_or_else(failure)?;
        let ready = snapshot(&row)?;
        if ready.state != GateState::Ready || ready.identity != identity {
            bail!("node gate result mismatch");
        }
        Ok(ready)
    }
}

/// Opens only app sessions, retaining each session's shared barrier until disconnection.
pub async fn connect_node_pool(
    database_url: &str,
    max_connections: u32,
    expected: GateSnapshot,
) -> Result<PgPool> {
    if expected.state != GateState::Ready {
        bail!("node pool requires a ready gate");
    }
    let attempt = super::connection::node_pool_options(max_connections)
        .after_connect(move |connection, _| {
            Box::pin(async move {
                let result: sqlx::Result<()> = async {
                    // These fixed bootstrap/session statements cannot use SQLx offline macros; no business SQL uses this exception.
                    let acquired: bool =
                        sqlx::query_scalar(include_str!("../sql/node_gate/try_shared.sql"))
                            .fetch_one(&mut *connection)
                            .await?;
                    if !acquired {
                        return Err(sqlx::Error::Protocol(
                            "node gate barrier unavailable".into(),
                        ));
                    }
                    let row = sqlx::query(include_str!("../sql/node_gate/read_admission.sql"))
                        .fetch_one(&mut *connection)
                        .await?;
                    let state: String = row.try_get("state")?;
                    let version: String = row.try_get("runtime_version")?;
                    let protocol: i32 = row.try_get("protocol_version")?;
                    let generation: i64 = row.try_get("generation")?;
                    let target: Option<String> = row.try_get("target_runtime_version")?;
                    let target_protocol: Option<i32> = row.try_get("target_protocol_version")?;
                    let owner: Option<Uuid> = row.try_get("owner_id")?;
                    let matches = state == "ready"
                        && target.is_none()
                        && target_protocol.is_none()
                        && owner.is_none()
                        && version.parse::<RuntimeVersion>().ok()
                            == Some(expected.identity.version)
                        && u32::try_from(protocol).ok() == Some(expected.identity.protocol)
                        && expected_admission(
                            expected,
                            GateSnapshot {
                                state: GateState::Ready,
                                identity: expected.identity,
                                generation,
                            },
                        );
                    if !matches {
                        return Err(sqlx::Error::Protocol("node gate admission denied".into()));
                    }
                    Ok(())
                }
                .await;
                result.map_err(|_| sqlx::Error::Protocol("node gate admission failed".into()))
            })
        })
        .connect(database_url);
    timeout(super::connection::POSTGRES_CONNECT_TIMEOUT, attempt)
        .await
        .map_err(|_| anyhow!("node pool connection timed out"))?
        .map_err(|_| failure())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(version: &str, protocol: u32) -> RuntimeIdentity {
        RuntimeIdentity {
            version: version.parse().unwrap(),
            protocol,
        }
    }

    #[test]
    fn versions_and_admission_are_numeric_and_fail_closed() {
        assert!("1.9.0".parse::<RuntimeVersion>().unwrap() < "1.10.0".parse().unwrap());
        for invalid in ["1.0", "1.0.0-rc1", "01.0.0", "1.0.0+build"] {
            assert!(invalid.parse::<RuntimeVersion>().is_err());
        }
        let mut gate = GateSnapshot {
            state: GateState::Ready,
            identity: identity("1.9.0", 1),
            generation: 1,
        };
        assert_eq!(
            decide_admission(identity("1.9.0", 1), gate),
            AdmissionDecision::Join
        );
        assert_eq!(
            decide_admission(identity("1.10.0", 1), gate),
            AdmissionDecision::Upgrade
        );
        assert_eq!(
            decide_admission(identity("1.8.0", 1), gate),
            AdmissionDecision::Reject
        );
        assert_eq!(
            decide_admission(identity("1.9.0", 2), gate),
            AdmissionDecision::Reject
        );
        assert_eq!(
            decide_admission(identity("1.10.0", 0), gate),
            AdmissionDecision::Reject
        );
        for state in [GateState::Draining, GateState::Migrating, GateState::Failed] {
            gate.state = state;
            assert_eq!(
                decide_admission(identity("1.10.0", 1), gate),
                AdmissionDecision::Block
            );
        }
    }

    #[test]
    fn pool_rejects_identity_generation_and_state_mismatches() {
        let expected = GateSnapshot {
            state: GateState::Ready,
            identity: identity("1.2.3", 1),
            generation: 4,
        };
        assert!(expected_admission(expected, expected));
        assert!(!expected_admission(
            expected,
            GateSnapshot {
                generation: 5,
                ..expected
            }
        ));
        assert!(!expected_admission(
            expected,
            GateSnapshot {
                identity: identity("1.2.4", 1),
                ..expected
            }
        ));
        assert!(!expected_admission(
            expected,
            GateSnapshot {
                identity: identity("1.2.3", 2),
                ..expected
            }
        ));
        assert!(!expected_admission(
            expected,
            GateSnapshot {
                state: GateState::Draining,
                ..expected
            }
        ));
        assert!(!expected_admission(
            GateSnapshot {
                state: GateState::Failed,
                ..expected
            },
            expected
        ));
    }

    #[test]
    fn schema_is_public_and_restricts_singleton() {
        let sql =
            entity::create_table_statement().to_string(sea_orm::sea_query::PostgresQueryBuilder);
        assert!(sql.contains("\"public\".\"prompt_ferry_node_gate\""));
        assert!(sql.contains("id = 1"));
    }

    #[test]
    fn lock_deadline_fails_closed() {
        let now = Instant::now();
        assert!(lock_wait_remaining(now, now).is_err());
        assert_eq!(
            lock_wait_remaining(now, now + Duration::from_secs(1)).unwrap(),
            Duration::from_secs(1)
        );
        assert!(lock_wait_remaining(now + Duration::from_secs(1), now).is_err());
    }

    #[test]
    fn legacy_bootstrap_requires_ack_and_no_live_owners() {
        assert_eq!(
            decide_bootstrap(false, false, false, false),
            BootstrapDecision::Allow
        );
        assert_eq!(
            decide_bootstrap(true, false, false, false),
            BootstrapDecision::RequireAcknowledgement
        );
        assert_eq!(
            decide_bootstrap(true, true, true, false),
            BootstrapDecision::LiveWorkers
        );
        assert_eq!(
            decide_bootstrap(true, true, false, true),
            BootstrapDecision::LiveRequestLeases
        );
        assert_eq!(
            decide_bootstrap(true, true, false, false),
            BootstrapDecision::Allow
        );
    }
}
