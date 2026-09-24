//! Issue #277 Phase P9 — PostgreSQL pool-wait observation.
//!
//! The worker lease pool serves request admission (lease heartbeats) and the
//! stale reconcile sweep. An exhausted pool used to surface only as a hard
//! acquire timeout; this module counts every acquire, counts the waits that
//! crossed [`POOL_WAIT_WARN`], and logs a `pool_wait` line for those waits so
//! contention is visible before it becomes a timeout.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use sqlx::{PgPool, Postgres, pool::PoolConnection};

/// A wait at or above this bound is counted and logged as contention.
pub const POOL_WAIT_WARN: Duration = Duration::from_millis(100);

static ACQUIRES: AtomicU64 = AtomicU64::new(0);
static SLOW_ACQUIRES: AtomicU64 = AtomicU64::new(0);
static TOTAL_WAIT_MS: AtomicU64 = AtomicU64::new(0);

/// Process-wide pool-wait counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolWaitSnapshot {
    pub acquires: u64,
    pub slow_acquires: u64,
    pub total_wait_ms: u64,
}

impl PoolWaitSnapshot {
    /// Difference from an earlier snapshot, used to scope a counter to one
    /// reconcile pass.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            acquires: self.acquires.saturating_sub(earlier.acquires),
            slow_acquires: self.slow_acquires.saturating_sub(earlier.slow_acquires),
            total_wait_ms: self.total_wait_ms.saturating_sub(earlier.total_wait_ms),
        }
    }
}

/// Read the current counters.
pub fn snapshot() -> PoolWaitSnapshot {
    PoolWaitSnapshot {
        acquires: ACQUIRES.load(Ordering::Relaxed),
        slow_acquires: SLOW_ACQUIRES.load(Ordering::Relaxed),
        total_wait_ms: TOTAL_WAIT_MS.load(Ordering::Relaxed),
    }
}

/// Acquire a pooled connection, recording how long the caller waited.
///
/// `path` names the caller (`lease_heartbeat`, `lease_reconcile`, …) so a slow
/// wait can be attributed in the log. A failed acquire still contributes its
/// wait: a pool that only ever times out must not look uncontended.
pub async fn acquire(
    pool: &PgPool,
    path: &'static str,
) -> Result<PoolConnection<Postgres>, sqlx::Error> {
    let started = Instant::now();
    let result = pool.acquire().await;
    record(path, started.elapsed(), result.is_err());
    result
}

fn record(path: &'static str, waited: Duration, failed: bool) {
    let waited_ms = u64::try_from(waited.as_millis()).unwrap_or(u64::MAX);
    let acquires = ACQUIRES.fetch_add(1, Ordering::Relaxed) + 1;
    TOTAL_WAIT_MS.fetch_add(waited_ms, Ordering::Relaxed);
    if failed || waited >= POOL_WAIT_WARN {
        let slow_acquires = SLOW_ACQUIRES.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::warn!(
            category = "pool_wait",
            pool_path = path,
            waited_ms,
            failed,
            acquires,
            slow_acquires,
            "postgres pool acquire waited on contention"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_reports_only_the_waits_observed_between_snapshots() {
        let start = PoolWaitSnapshot {
            acquires: 4,
            slow_acquires: 1,
            total_wait_ms: 30,
        };
        let later = PoolWaitSnapshot {
            acquires: 6,
            slow_acquires: 2,
            total_wait_ms: 155,
        };
        assert_eq!(
            later.since(start),
            PoolWaitSnapshot {
                acquires: 2,
                slow_acquires: 1,
                total_wait_ms: 125,
            }
        );
        assert_eq!(start.since(later), PoolWaitSnapshot::default());
    }
}
