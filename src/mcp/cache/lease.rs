//! Cross-worker ownership for one MCP catalog refresh.
//!
//! Exactly one worker may refresh a given server at a time. Valkey owns that
//! decision with `SET NX EX` and releases it with a token-safe
//! compare-and-delete, SQLite reuses the standalone coordinator leases, and
//! the in-memory backend falls back to an in-process single-flight registry
//! because no shared coordination is available there.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use redis::aio::ConnectionManager;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use super::{CacheBackend, CatalogActivity, SqliteCatalogBackend};

/// The Valkey key namespace of one refresh lease.
pub(super) const REFRESH_KEY_PREFIX: &str = "pfy:mcp-catalog-refresh:";

/// Release the lease only when the stored token still matches the owner, so
/// an expired lease that another worker already took over stays untouched.
const RELEASE_IF_OWNER_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
    return redis.call('DEL', KEYS[1])
end
return 0
"#;

/// The in-process single-flight registry used when no shared coordination
/// backend is configured.
///
/// Each slot carries the owner token of the refresh that took it, so a release
/// never removes a successor's slot. The slot also expires on its own, so a
/// cancelled refresh cannot wedge the server forever.
#[derive(Clone, Debug, Default)]
pub(super) struct LocalRefreshRegistry {
    entries: Arc<RwLock<HashMap<Uuid, LocalLease>>>,
}

#[derive(Clone, Copy, Debug)]
struct LocalLease {
    owner: Uuid,
    deadline: Instant,
}

impl LocalRefreshRegistry {
    /// Take the slot for one server and return its owner token.
    pub(super) async fn try_acquire(
        &self,
        server_id: Uuid,
        owner: Uuid,
        now: Instant,
        ttl: Duration,
    ) -> bool {
        let mut entries = self.entries.write().await;
        entries.retain(|_, lease| lease.deadline > now);
        if entries.contains_key(&server_id) {
            return false;
        }
        entries.insert(
            server_id,
            LocalLease {
                owner,
                deadline: now + ttl,
            },
        );
        true
    }

    /// Release the slot only when `owner` still holds it.
    ///
    /// An owner whose lease already expired and whose successor took the slot
    /// leaves the successor alone.
    pub(super) async fn release(&self, server_id: Uuid, owner: Uuid) {
        let mut entries = self.entries.write().await;
        if entries
            .get(&server_id)
            .is_some_and(|lease| lease.owner == owner)
        {
            entries.remove(&server_id);
        }
    }
}

#[derive(Clone)]
enum LeaseKind {
    Local {
        registry: LocalRefreshRegistry,
        server_id: Uuid,
        owner: Uuid,
    },
    Valkey {
        manager: ConnectionManager,
        key: String,
        token: String,
    },
    Sqlite {
        store: SqliteCatalogBackend,
        key: String,
        owner: String,
    },
}

/// The default cleanup budget of a drop-initiated lease release.
const DEFAULT_CLEANUP_BUDGET: Duration = Duration::from_secs(5);

/// The ownership token of one in-flight catalog refresh.
///
/// The lease carries a tracked activity clone, so an aborted caller cannot drop
/// out of drain accounting before the release cleanup finishes.
pub(crate) struct RefreshLease {
    inner: Option<LeaseKind>,
    activity: Option<CatalogActivity>,
    cleanup_budget: Duration,
}

impl RefreshLease {
    fn new(inner: LeaseKind) -> Self {
        Self {
            inner: Some(inner),
            activity: None,
            cleanup_budget: DEFAULT_CLEANUP_BUDGET,
        }
    }

    /// Attach the tracked operation that must outlive the release cleanup.
    pub(super) fn track(mut self, activity: CatalogActivity, cleanup_budget: Duration) -> Self {
        self.activity = Some(activity);
        self.cleanup_budget = cleanup_budget;
        self
    }

    /// Release the lease cooperatively.
    ///
    /// The release is token-safe: a lease that already expired and was taken
    /// over by another worker is left alone. The token stays owned by this
    /// guard until the backend release completes, so a cancelled release hands
    /// the cleanup responsibility to this guard's drop instead of losing the
    /// token. The caller bounds this wait with the cleanup margin.
    pub(crate) async fn release(mut self) {
        let Some(kind) = self.inner.clone() else {
            return;
        };
        Self::release_kind(kind).await;
        self.inner = None;
    }

    /// Perform one token-safe backend release.
    ///
    /// Deliberately free of the guard, so a bounded cleanup task cannot
    /// re-enter `Drop` and spawn another cleanup round.
    async fn release_kind(inner: LeaseKind) {
        match inner {
            LeaseKind::Local {
                registry,
                server_id,
                owner,
            } => registry.release(server_id, owner).await,
            LeaseKind::Valkey {
                manager,
                key,
                token,
            } => {
                let mut manager = manager;
                match redis::Script::new(RELEASE_IF_OWNER_SCRIPT)
                    .key(&key)
                    .arg(&token)
                    .invoke_async::<i64>(&mut manager)
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        warn!(error = %err, "failed to release the MCP catalog refresh lease");
                    }
                }
            }
            LeaseKind::Sqlite { store, key, owner } => {
                if let Err(err) = store.coordinator.release_lease(&key, &owner).await {
                    warn!(error = %err, "failed to release the SQLite MCP catalog refresh lease");
                }
            }
        }
    }
}

impl Drop for RefreshLease {
    fn drop(&mut self) {
        // A cancelled or aborted refresh must not keep the lease, and the
        // cleanup must stay visible to the drain. The release runs in a bounded
        // task that holds the tracked activity clone until it exits; without a
        // runtime the activity is released and the lease expiry recovers.
        let Some(inner) = self.inner.take() else {
            return;
        };
        let activity = self.activity.take();
        let budget = self.cleanup_budget;
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                budget_ms = budget.as_millis() as u64,
                "the MCP catalog refresh lease is released only by its expiry: no runtime for cleanup"
            );
            drop(activity);
            return;
        };
        handle.spawn(async move {
            // The token is already taken from this guard, so a timed-out
            // cleanup reports expiry recovery instead of spawning another
            // cleanup round.
            if tokio::time::timeout(budget, RefreshLease::release_kind(inner))
                .await
                .is_err()
            {
                warn!(
                    budget_ms = budget.as_millis() as u64,
                    "the MCP catalog refresh lease cleanup exceeded its margin; the lease is recovered only by its expiry"
                );
            }
            drop(activity);
        });
    }
}

/// Take the refresh lease for one server.
///
/// `None` means another worker already owns the refresh. A coordination
/// backend that cannot be reached falls back to the in-process single-flight
/// registry: the refresh still runs, but only one process deduplicates it.
pub(super) async fn try_acquire(
    backend: &CacheBackend,
    registry: &LocalRefreshRegistry,
    server_id: Uuid,
    ttl: Duration,
) -> Option<RefreshLease> {
    match backend {
        CacheBackend::Local => acquire_local(registry, server_id, ttl).await,
        CacheBackend::Valkey { manager, .. } => {
            match acquire_valkey(manager, server_id, ttl).await {
                Ok(lease) => lease,
                Err(err) => {
                    warn!(
                        error = %err,
                        server_id = %server_id,
                        "shared MCP catalog refresh coordination is unavailable; \
                         falling back to in-process coordination"
                    );
                    acquire_local(registry, server_id, ttl).await
                }
            }
        }
        CacheBackend::Sqlite(store) => match acquire_sqlite(store, server_id, ttl).await {
            Ok(lease) => lease,
            Err(err) => {
                warn!(
                    error = %err,
                    server_id = %server_id,
                    "shared MCP catalog refresh coordination is unavailable; \
                     falling back to in-process coordination"
                );
                acquire_local(registry, server_id, ttl).await
            }
        },
    }
}

async fn acquire_local(
    registry: &LocalRefreshRegistry,
    server_id: Uuid,
    ttl: Duration,
) -> Option<RefreshLease> {
    let owner = Uuid::new_v4();
    if !registry
        .try_acquire(server_id, owner, Instant::now(), ttl)
        .await
    {
        return None;
    }
    Some(RefreshLease::new(LeaseKind::Local {
        registry: registry.clone(),
        server_id,
        owner,
    }))
}

async fn acquire_valkey(
    manager: &ConnectionManager,
    server_id: Uuid,
    ttl: Duration,
) -> anyhow::Result<Option<RefreshLease>> {
    let key = format!("{REFRESH_KEY_PREFIX}{server_id}");
    let token = Uuid::new_v4().to_string();
    let mut manager = manager.clone();
    let acquired = redis::cmd("SET")
        .arg(&key)
        .arg(&token)
        .arg("NX")
        .arg("EX")
        .arg(ttl.as_secs().max(1))
        .query_async::<Option<String>>(&mut manager)
        .await?;
    Ok(acquired.map(|_| {
        RefreshLease::new(LeaseKind::Valkey {
            manager,
            key,
            token,
        })
    }))
}

async fn acquire_sqlite(
    store: &SqliteCatalogBackend,
    server_id: Uuid,
    ttl: Duration,
) -> anyhow::Result<Option<RefreshLease>> {
    let key = format!("{REFRESH_KEY_PREFIX}{server_id}");
    let owner = Uuid::new_v4().to_string();
    let acquired = store
        .coordinator
        .acquire_lease(&key, &owner, ttl.as_secs().max(1))
        .await?;
    Ok(acquired.then_some(RefreshLease::new(LeaseKind::Sqlite {
        store: store.clone(),
        key,
        owner,
    })))
}
