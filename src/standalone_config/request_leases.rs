use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::standalone_config::Result;

/// Outcome of a `acquire` call against `standalone_request_leases`.
/// The store distinguishes "applied" from "blocked" so the runtime
/// guard can decide whether to keep heartbeating or surface a take-over
/// to its caller without performing a second read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestLeaseAcquireOutcome {
    /// The row was either freshly inserted or refreshed by the same
    /// owner; the caller should proceed to heartbeat.
    Acquired,
    /// A live row owned by a different worker instance still holds the
    /// lease; the caller must not heartbeat or delete it.
    Blocked,
}

/// In-memory view of an active standalone request lease row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveRequestLease {
    pub(crate) request_id: Uuid,
    pub(crate) owner_worker_id: Uuid,
    pub(crate) lease_expires_at: i64,
    pub(crate) last_heartbeat_at: i64,
}

/// Owner-checked SQLite store for standalone request leases.
///
/// Phase 1C-c ships the dedicated `standalone_request_leases` table
/// keyed by request id. Acquire may take over an expired lease, but
/// refresh and release remain owner-checked so an older process cannot
/// mutate or delete a newer owner's row. The stale reconciler only
/// deletes expired rows because standalone request records do not exist
/// yet; it must never be presented as aborting a durable request.
#[derive(Clone, Debug)]
pub(crate) struct StandaloneRequestLeaseStore {
    pool: SqlitePool,
}

impl StandaloneRequestLeaseStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(crate) async fn acquire(
        &self,
        request_id: Uuid,
        owner_worker_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<RequestLeaseAcquireOutcome> {
        let now = unix_seconds();
        let expires_at = now.saturating_add(ttl_seconds.max(1) as i64);
        let row = standalone_query!("src/sql/standalone/request_lease_acquire.sql")
            .bind(request_id.to_string())
            .bind(owner_worker_id.to_string())
            .bind(expires_at)
            .bind(now)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            // The `ON CONFLICT ... WHERE` gate rejected the upsert
            // because the existing row is owned by a different worker
            // and has not yet expired.
            return Ok(RequestLeaseAcquireOutcome::Blocked);
        };
        let owner: String = row.try_get("owner_worker_id")?;
        if owner == owner_worker_id.to_string() {
            Ok(RequestLeaseAcquireOutcome::Acquired)
        } else {
            // The returned owner must always equal the bind owner when
            // the upsert applied; report blocked conservatively if a
            // future SQLite change ever returns the prior owner.
            Ok(RequestLeaseAcquireOutcome::Blocked)
        }
    }

    /// Owner-checked refresh: the row must exist, belong to `owner`, and
    /// not be expired. Returns `true` when the row was advanced.
    pub(crate) async fn refresh(
        &self,
        request_id: Uuid,
        owner_worker_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<bool> {
        let now = unix_seconds();
        let expires_at = now.saturating_add(ttl_seconds.max(1) as i64);
        let result = standalone_query!("src/sql/standalone/request_lease_refresh.sql")
            .bind(expires_at)
            .bind(now)
            .bind(now)
            .bind(request_id.to_string())
            .bind(owner_worker_id.to_string())
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Owner-checked release: deletes only when the row exists and
    /// belongs to `owner`. Returns `true` when a row was deleted.
    pub(crate) async fn release(&self, request_id: Uuid, owner_worker_id: Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/request_lease_release.sql")
            .bind(request_id.to_string())
            .bind(owner_worker_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Return all currently live leases (rows whose `lease_expires_at`
    /// is strictly after the bind-time `now` snapshot).
    pub(crate) async fn list_active(&self) -> Result<Vec<ActiveRequestLease>> {
        let now = unix_seconds();
        let rows = standalone_query!("src/sql/standalone/request_lease_list_active.sql")
            .bind(now)
            .fetch_all(&self.pool)
            .await?;
        let mut leases = Vec::with_capacity(rows.len());
        for row in rows {
            let request_id: String = row.try_get("request_id")?;
            let request_id = Uuid::parse_str(&request_id).map_err(|error| {
                crate::standalone_config::StandaloneConfigError::CorruptDatabase(format!(
                    "column request_id is not a UUID: {error}"
                ))
            })?;
            let owner: String = row.try_get("owner_worker_id")?;
            let owner = Uuid::parse_str(&owner).map_err(|error| {
                crate::standalone_config::StandaloneConfigError::CorruptDatabase(format!(
                    "column owner_worker_id is not a UUID: {error}"
                ))
            })?;
            let lease_expires_at: i64 = row.try_get("lease_expires_at")?;
            let last_heartbeat_at: i64 = row.try_get("last_heartbeat_at")?;
            leases.push(ActiveRequestLease {
                request_id,
                owner_worker_id: owner,
                lease_expires_at,
                last_heartbeat_at,
            });
        }
        Ok(leases)
    }

    /// Delete every lease row whose `lease_expires_at` is at or before
    /// the bind-time `now` snapshot. The reconciler must not be
    /// described as aborting a durable request record; standalone
    /// request records do not yet exist.
    pub(crate) async fn abort_stale(&self) -> Result<u64> {
        let now = unix_seconds();
        let result = standalone_query!("src/sql/standalone/request_lease_abort_stale.sql")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
