use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use crate::{config::WorkerConfig, db::McpServer};

use super::freshness::{CatalogFreshness, CatalogFreshnessPolicy, content_fingerprint};
use lease::LocalRefreshRegistry;

mod coordinator;
mod lease;

pub(crate) use coordinator::{CatalogActivity, CatalogRefreshCoordinator, RefreshReservation};
pub(crate) use lease::RefreshLease;

pub const MCP_CATALOG_VALKEY_KEY_PREFIX: &str = "pfy:mcp-catalog:";

/// The coordinator namespace of one stored catalog envelope.
const CATALOG_NAMESPACE: &str = "mcp-catalog";

/// Atomically read the current catalog envelope, initializing an absent one
/// with a tombstone so every peer shares the same baseline.
const VALKEY_BASELINE_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if not current then
    redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
    return ARGV[1]
end
redis.call('EXPIRE', KEYS[1], ARGV[2])
return current
"#;

/// Replace the catalog envelope only while it still equals the baseline.
const VALKEY_PUBLISH_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
    redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[3])
    return 1
end
return 0
"#;

/// Bounded wait for the initial Valkey handshake. Without this wrapper
/// `get_connection_manager` follows the OS TCP timeout (~60-120s) and
/// the worker startup hangs even though the rest of the admin state
/// is already ready; on timeout we fall back to the local cache so
/// requests still drain.
const VALKEY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerCatalogSnapshot {
    pub tools: Vec<Value>,
    pub resources: Vec<Value>,
    #[serde(default)]
    pub resource_templates: Vec<Value>,
    pub prompts: Vec<Value>,
}

/// The result of one catalog lookup: the freshness state plus the snapshot
/// that state belongs to.
#[derive(Clone, Debug)]
pub struct CatalogLookup {
    pub freshness: CatalogFreshness,
    pub snapshot: Option<ServerCatalogSnapshot>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    /// The identity of the publication this snapshot came from.
    ///
    /// Regenerated on every successful publication, even when the content did
    /// not change, so a follower can prove that a refresh it was waiting for
    /// actually published instead of observing an older snapshot.
    pub publication_id: Option<String>,
}

impl CatalogLookup {
    fn missing() -> Self {
        Self {
            freshness: CatalogFreshness::Missing,
            snapshot: None,
            fetched_at: None,
            content_hash: None,
            publication_id: None,
        }
    }
}

#[derive(Clone, Debug)]
struct CacheEntry {
    updated_at: DateTime<Utc>,
    fetched_at: Option<DateTime<Utc>>,
    content_hash: Option<String>,
    publication_id: Option<String>,
    snapshot: ServerCatalogSnapshot,
}

/// One in-process cell: its publication revision and its current envelope.
///
/// The revision is the local backend's publication fence. Invalidation and
/// publication advance it under the same lock, so a publisher that captured an
/// older revision cannot overwrite a newer state.
#[derive(Clone, Debug, Default)]
struct LocalSlot {
    revision: u64,
    entry: Option<CacheEntry>,
}

/// The exact catalog state a refresh observed before its fetch.
#[derive(Clone, Debug)]
pub(crate) enum BaselineToken {
    /// The local publication revision.
    Local(u64),
    /// The shared envelope payload, compared byte-for-byte by the backend CAS.
    Shared(String),
}

/// The baseline envelope plus the lookup derived from it.
pub(crate) struct CatalogBaseline {
    pub(crate) token: BaselineToken,
    pub(crate) lookup: CatalogLookup,
}

/// The stored catalog envelope.
///
/// `fetched_at`, `content_hash` and `publication_id` were added after the first
/// version of this payload, so all three stay optional: a payload without an
/// acquisition timestamp is expired, never fresh.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredCatalog {
    updated_at: DateTime<Utc>,
    #[serde(default)]
    fetched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    content_hash: Option<String>,
    #[serde(default)]
    publication_id: Option<String>,
    snapshot: ServerCatalogSnapshot,
}

#[derive(Clone)]
pub(super) enum CacheBackend {
    Local,
    Valkey {
        manager: ConnectionManager,
        ttl_seconds: u64,
    },
    Sqlite(SqliteCatalogBackend),
}

#[derive(Clone)]
pub(super) struct SqliteCatalogBackend {
    pub(super) coordinator: crate::standalone_config::StandaloneCoordinatorStore,
    pub(super) ttl_seconds: u64,
}

#[derive(Clone)]
pub struct McpCatalogCache {
    backend: CacheBackend,
    local: Arc<RwLock<HashMap<Uuid, LocalSlot>>>,
    policy: CatalogFreshnessPolicy,
    refresh_registry: LocalRefreshRegistry,
    failure_backoff: Arc<RwLock<HashMap<(Uuid, DateTime<Utc>), Instant>>>,
    coordinator: Arc<CatalogRefreshCoordinator>,
}

impl McpCatalogCache {
    pub fn new() -> Self {
        Self::with_freshness_policy(CatalogFreshnessPolicy::production())
    }

    /// Build a cache with an explicit serving-age policy. Production uses
    /// [`CatalogFreshnessPolicy::production`]; tests use a compressed window.
    pub fn with_freshness_policy(policy: CatalogFreshnessPolicy) -> Self {
        Self {
            backend: CacheBackend::Local,
            local: Arc::new(RwLock::new(HashMap::new())),
            policy,
            refresh_registry: LocalRefreshRegistry::default(),
            failure_backoff: Arc::new(RwLock::new(HashMap::new())),
            coordinator: Arc::new(CatalogRefreshCoordinator::new()),
        }
    }

    pub fn freshness_policy(&self) -> CatalogFreshnessPolicy {
        self.policy
    }

    /// The refresh coordinator shared by every service wrapper of this cache.
    pub(crate) fn refresh_coordinator(&self) -> &Arc<CatalogRefreshCoordinator> {
        &self.coordinator
    }

    /// Adopt the refresh bound of the service policy driving this cache.
    pub(crate) fn configure_refresh_concurrency(&self, concurrency: usize) {
        self.coordinator.configure_capacity(concurrency);
    }

    pub async fn from_config(config: &WorkerConfig) -> Self {
        Self::from_config_with_sqlite(config, None).await
    }

    pub async fn from_config_with_sqlite(
        config: &WorkerConfig,
        sqlite_pool: Option<sqlx::SqlitePool>,
    ) -> Self {
        let url = config.valkey_url.trim();
        if url.is_empty() {
            if let Some(pool) = sqlite_pool {
                return Self::sqlite(pool, config.valkey_ttl_seconds, "valkey_not_configured");
            }
            return Self::new();
        }

        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to open valkey client for MCP catalog");
                if let Some(pool) = sqlite_pool {
                    return Self::sqlite(
                        pool,
                        config.valkey_ttl_seconds,
                        "valkey_client_open_failed",
                    );
                }
                return Self::new();
            }
        };
        let manager = match tokio::time::timeout(
            VALKEY_CONNECT_TIMEOUT,
            client.get_connection_manager(),
        )
        .await
        {
            Ok(Ok(manager)) => manager,
            Ok(Err(err)) => {
                warn!(error = %err, valkey_url = url, "failed to connect valkey for MCP catalog");
                if let Some(pool) = sqlite_pool {
                    return Self::sqlite(
                        pool,
                        config.valkey_ttl_seconds,
                        "valkey_connection_failed",
                    );
                }
                return Self::new();
            }
            Err(_) => {
                warn!(
                    timeout_seconds = VALKEY_CONNECT_TIMEOUT.as_secs(),
                    valkey_url = url,
                    "timed out connecting to valkey for MCP catalog; falling back to local cache",
                );
                if let Some(pool) = sqlite_pool {
                    return Self::sqlite(
                        pool,
                        config.valkey_ttl_seconds,
                        "valkey_connection_timeout",
                    );
                }
                return Self::new();
            }
        };

        Self {
            backend: CacheBackend::Valkey {
                manager,
                ttl_seconds: config.valkey_ttl_seconds.max(1),
            },
            local: Arc::new(RwLock::new(HashMap::new())),
            policy: CatalogFreshnessPolicy::production(),
            refresh_registry: LocalRefreshRegistry::default(),
            failure_backoff: Arc::new(RwLock::new(HashMap::new())),
            coordinator: Arc::new(CatalogRefreshCoordinator::new()),
        }
    }

    fn sqlite(pool: sqlx::SqlitePool, ttl_seconds: u64, reason: &str) -> Self {
        warn!(
            backend = "sqlite",
            reason,
            scope = "single-host",
            network_filesystem_safe = false,
            "using WAL-backed SQLite MCP catalog cache"
        );
        Self {
            backend: CacheBackend::Sqlite(SqliteCatalogBackend {
                coordinator: crate::standalone_config::StandaloneCoordinatorStore::new(pool),
                ttl_seconds: ttl_seconds.max(1),
            }),
            local: Arc::new(RwLock::new(HashMap::new())),
            policy: CatalogFreshnessPolicy::production(),
            refresh_registry: LocalRefreshRegistry::default(),
            failure_backoff: Arc::new(RwLock::new(HashMap::new())),
            coordinator: Arc::new(CatalogRefreshCoordinator::new()),
        }
    }

    /// Read one catalog and classify it against the serving-age policy.
    ///
    /// A read never changes the acquisition timestamp: an old snapshot stays
    /// old, so it can never be mistaken for a fresh one.
    pub async fn lookup(&self, server: &McpServer) -> CatalogLookup {
        let Some(stored) = self.read_stored(server).await else {
            return CatalogLookup::missing();
        };
        self.classify(stored)
    }

    fn classify(&self, stored: StoredCatalog) -> CatalogLookup {
        let content_hash = stored
            .content_hash
            .clone()
            .unwrap_or_else(|| content_fingerprint(&stored.snapshot));
        let freshness = self.policy.evaluate(stored.fetched_at, Utc::now());
        CatalogLookup {
            freshness,
            snapshot: Some(stored.snapshot),
            fetched_at: stored.fetched_at,
            content_hash: Some(content_hash),
            publication_id: stored.publication_id,
        }
    }

    /// The raw cached snapshot without any freshness decision.
    ///
    /// Reserved for diagnostics and tests: every reader that serves a
    /// catalog goes through [`Self::lookup`].
    pub async fn raw_snapshot(&self, server: &McpServer) -> Option<ServerCatalogSnapshot> {
        self.lookup(server).await.snapshot
    }

    /// Capture the exact catalog state a refresh is based on.
    ///
    /// For a shared backend this atomically initializes an absent key with a
    /// revisioned tombstone, so every peer that observes "no snapshot" shares
    /// the same baseline payload. A later publication may replace that payload
    /// only while it is still current.
    pub(crate) async fn baseline(&self, server: &McpServer) -> anyhow::Result<CatalogBaseline> {
        match &self.backend {
            CacheBackend::Local => {
                let slot = self.local_slot(server.server_id).await;
                let lookup = slot
                    .entry
                    .filter(|entry| entry.updated_at == server.updated_at)
                    .map(|entry| self.classify(stored_from_entry(entry)))
                    .unwrap_or_else(CatalogLookup::missing);
                Ok(CatalogBaseline {
                    token: BaselineToken::Local(slot.revision),
                    lookup,
                })
            }
            CacheBackend::Sqlite(store) => {
                let key = server.server_id.to_string();
                let payload = store
                    .coordinator
                    .get_or_insert(
                        CATALOG_NAMESPACE,
                        &key,
                        &tombstone_payload(),
                        store.ttl_seconds,
                    )
                    .await
                    .map_err(|err| anyhow!("failed to read the SQLite catalog baseline: {err}"))?;
                let lookup = self.adopt_shared(&payload, server).await;
                Ok(CatalogBaseline {
                    token: BaselineToken::Shared(payload),
                    lookup,
                })
            }
            CacheBackend::Valkey {
                manager,
                ttl_seconds,
            } => {
                let payload = valkey_baseline(manager, server.server_id, *ttl_seconds)
                    .await
                    .map_err(|err| anyhow!("failed to read the Valkey catalog baseline: {err}"))?;
                let lookup = self.adopt_shared(&payload, server).await;
                Ok(CatalogBaseline {
                    token: BaselineToken::Shared(payload),
                    lookup,
                })
            }
        }
    }

    /// Store a freshly fetched catalog for the current configuration
    /// generation.
    ///
    /// Unconditional: reserved for seeding a snapshot outside a coordinated
    /// refresh. A refresh publishes through [`Self::publish_if_current`].
    pub async fn put(&self, server: &McpServer, snapshot: ServerCatalogSnapshot) {
        self.put_at(server, snapshot, Utc::now()).await;
    }

    /// Store a catalog with an explicit acquisition timestamp.
    pub async fn put_at(
        &self,
        server: &McpServer,
        snapshot: ServerCatalogSnapshot,
        fetched_at: DateTime<Utc>,
    ) {
        let stored = self.stored_from(server, &snapshot, fetched_at);
        self.insert_local(server.server_id, stored_from_stored(&stored))
            .await;
        self.write_shared(server.server_id, &stored).await;
    }

    /// Publish a fetched snapshot only while the baseline is still current.
    ///
    /// Returns `false` when another peer invalidated or published meanwhile.
    /// The shared backend decides this atomically; the local backend compares
    /// the publication revision under its own lock. A failed compare leaves
    /// both the shared entry and this cache's local view untouched.
    pub(crate) async fn publish_if_current(
        &self,
        server: &McpServer,
        token: &BaselineToken,
        snapshot: ServerCatalogSnapshot,
        fetched_at: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        let stored = self.stored_from(server, &snapshot, fetched_at);
        match (&self.backend, token) {
            (CacheBackend::Local, BaselineToken::Local(revision)) => {
                let mut slots = self.local.write().await;
                let slot = slots.entry(server.server_id).or_default();
                if slot.revision != *revision {
                    return Ok(false);
                }
                slot.revision = slot.revision.wrapping_add(1);
                slot.entry = Some(stored_from_stored(&stored));
                Ok(true)
            }
            (CacheBackend::Sqlite(store), BaselineToken::Shared(expected)) => {
                let payload = serde_json::to_string(&stored)?;
                let replaced = store
                    .coordinator
                    .replace_if_current(
                        CATALOG_NAMESPACE,
                        &server.server_id.to_string(),
                        expected,
                        &payload,
                        store.ttl_seconds,
                    )
                    .await?;
                if replaced {
                    self.insert_local(server.server_id, stored_from_stored(&stored))
                        .await;
                }
                Ok(replaced)
            }
            (
                CacheBackend::Valkey {
                    manager,
                    ttl_seconds,
                },
                BaselineToken::Shared(expected),
            ) => {
                let payload = serde_json::to_string(&stored)?;
                let replaced =
                    valkey_publish(manager, server.server_id, expected, &payload, *ttl_seconds)
                        .await?;
                if replaced {
                    self.insert_local(server.server_id, stored_from_stored(&stored))
                        .await;
                }
                Ok(replaced)
            }
            _ => Ok(false),
        }
    }

    /// Drop one catalog and fence every in-flight publication for it.
    ///
    /// A shared backend writes a new revisioned tombstone instead of deleting
    /// the key, so a publisher that captured the old payload fails its compare
    /// and cannot resurrect the invalidated snapshot.
    pub async fn invalidate(&self, server_id: Uuid) {
        {
            let mut slots = self.local.write().await;
            let slot = slots.entry(server_id).or_default();
            slot.revision = slot.revision.wrapping_add(1);
            slot.entry = None;
        }
        self.failure_backoff
            .write()
            .await
            .retain(|(backoff_server_id, _), _| *backoff_server_id != server_id);

        let tombstone = tombstone_payload();
        match &self.backend {
            CacheBackend::Local => {}
            CacheBackend::Sqlite(store) => {
                if let Err(err) = store
                    .coordinator
                    .put(
                        CATALOG_NAMESPACE,
                        &server_id.to_string(),
                        &tombstone,
                        store.ttl_seconds,
                    )
                    .await
                {
                    warn!(error = %err, server_id = %server_id, "failed to write the MCP catalog tombstone to SQLite");
                }
            }
            CacheBackend::Valkey {
                manager,
                ttl_seconds,
            } => {
                let mut manager = manager.clone();
                if let Err(err) = manager
                    .set_ex::<_, _, ()>(catalog_key(server_id), tombstone, *ttl_seconds)
                    .await
                {
                    warn!(error = %err, server_id = %server_id, "failed to write the MCP catalog tombstone to valkey");
                }
            }
        }
    }

    /// Take the refresh lease for one server.
    ///
    /// The lease is the cross-worker ownership token for one catalog refresh:
    /// Valkey uses `SET NX EX` with a token-safe compare-and-delete release,
    /// SQLite reuses the standalone coordinator leases, and the in-memory
    /// backend falls back to an in-process single-flight registry because no
    /// shared coordination is available there.
    pub(crate) async fn try_acquire_refresh_lease(
        &self,
        server_id: Uuid,
        ttl: Duration,
    ) -> Option<RefreshLease> {
        lease::try_acquire(&self.backend, &self.refresh_registry, server_id, ttl).await
    }

    /// Take the refresh lease and hand it a tracked cleanup activity.
    ///
    /// Production owners use this path so an aborted caller keeps the
    /// operation visible to the drain until the bounded release finishes.
    pub(crate) async fn try_acquire_tracked_refresh_lease(
        &self,
        server_id: Uuid,
        ttl: Duration,
        cleanup_budget: Duration,
        activity: CatalogActivity,
    ) -> Option<RefreshLease> {
        self.try_acquire_refresh_lease(server_id, ttl)
            .await
            .map(|lease| lease.track(activity, cleanup_budget))
    }
}

impl Default for McpCatalogCache {
    fn default() -> Self {
        Self::new()
    }
}

impl McpCatalogCache {
    /// Whether a recent failed refresh still suppresses the next background
    /// attempt for the same server configuration generation.
    ///
    /// The backoff is bounded and never blocks a read that has to recover an
    /// expired catalog; it only keeps the background pass from hammering an
    /// upstream that just failed.
    pub(crate) async fn refresh_backoff_active(
        &self,
        server_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> bool {
        let now = Instant::now();
        let mut entries = self.failure_backoff.write().await;
        entries.retain(|_, deadline| *deadline > now);
        entries.contains_key(&(server_id, updated_at))
    }

    pub(crate) async fn record_refresh_failure(
        &self,
        server_id: Uuid,
        updated_at: DateTime<Utc>,
        backoff: Duration,
    ) {
        self.failure_backoff
            .write()
            .await
            .insert((server_id, updated_at), Instant::now() + backoff);
    }

    pub(crate) async fn clear_refresh_failure(&self, server_id: Uuid) {
        self.failure_backoff
            .write()
            .await
            .retain(|(backoff_server_id, _), _| *backoff_server_id != server_id);
    }
}

impl McpCatalogCache {
    async fn read_stored(&self, server: &McpServer) -> Option<StoredCatalog> {
        match &self.backend {
            CacheBackend::Local => self.local_stored(server).await,
            CacheBackend::Valkey { manager, .. } => {
                let key = catalog_key(server.server_id);
                let mut manager = manager.clone();
                match manager.get::<_, Option<String>>(&key).await {
                    Ok(Some(value)) => self.adopt_shared_ref(&value, server).await,
                    // Absent and tombstoned entries both mean "no catalog"; an
                    // invalidation is visible to every instance, so the local
                    // copy must not be served either.
                    Ok(None) => {
                        self.clear_local(server.server_id).await;
                        None
                    }
                    // The shared authority could not be established. Serving
                    // the local copy could resurrect a snapshot a peer already
                    // invalidated, so this read fails closed and reports the
                    // catalog as missing; recovery then fails at the shared
                    // baseline instead of serving stale data.
                    Err(err) => {
                        warn!(error = %err, server_id = %server.server_id, "failed to read MCP catalog from valkey; the shared authority is unavailable");
                        self.clear_local(server.server_id).await;
                        None
                    }
                }
            }
            CacheBackend::Sqlite(store) => {
                match store
                    .coordinator
                    .get(CATALOG_NAMESPACE, &server.server_id.to_string())
                    .await
                {
                    Ok(Some(value)) => self.adopt_shared_ref(&value, server).await,
                    Ok(None) => {
                        self.clear_local(server.server_id).await;
                        None
                    }
                    // See the Valkey branch: a shared backend that cannot be
                    // read must not fall back to a local copy whose current
                    // revision is unknown.
                    Err(err) => {
                        warn!(error = %err, server_id = %server.server_id, "failed to read MCP catalog from SQLite; the shared authority is unavailable");
                        self.clear_local(server.server_id).await;
                        None
                    }
                }
            }
        }
    }

    /// Adopt a shared payload as the local view and return its envelope.
    ///
    /// A tombstone, an unparsable payload, or one of another configuration
    /// generation means the local copy is no longer authoritative.
    async fn adopt_shared_ref(&self, value: &str, server: &McpServer) -> Option<StoredCatalog> {
        let stored = self.parse_stored(value, server);
        match &stored {
            Some(stored) => {
                self.insert_local(server.server_id, stored_from_stored(stored))
                    .await
            }
            None => self.clear_local(server.server_id).await,
        }
        stored
    }

    /// Adopt a shared payload and return the lookup it represents.
    async fn adopt_shared(&self, value: &str, server: &McpServer) -> CatalogLookup {
        self.adopt_shared_ref(value, server)
            .await
            .map(|stored| self.classify(stored))
            .unwrap_or_else(CatalogLookup::missing)
    }

    fn parse_stored(&self, value: &str, server: &McpServer) -> Option<StoredCatalog> {
        if is_tombstone(value) {
            return None;
        }
        let stored: StoredCatalog = match serde_json::from_str(value) {
            Ok(stored) => stored,
            Err(err) => {
                warn!(error = %err, server_id = %server.server_id, "invalid MCP catalog cache payload");
                return None;
            }
        };
        if stored.updated_at != server.updated_at {
            return None;
        }
        Some(stored)
    }

    async fn local_slot(&self, server_id: Uuid) -> LocalSlot {
        self.local
            .read()
            .await
            .get(&server_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn local_stored(&self, server: &McpServer) -> Option<StoredCatalog> {
        self.local_slot(server.server_id)
            .await
            .entry
            .filter(|entry| entry.updated_at == server.updated_at)
            .map(stored_from_entry)
    }

    async fn clear_local(&self, server_id: Uuid) {
        let mut slots = self.local.write().await;
        let slot = slots.entry(server_id).or_default();
        if slot.entry.take().is_some() {
            slot.revision = slot.revision.wrapping_add(1);
        }
    }

    async fn insert_local(&self, server_id: Uuid, entry: CacheEntry) {
        let mut slots = self.local.write().await;
        let slot = slots.entry(server_id).or_default();
        slot.revision = slot.revision.wrapping_add(1);
        slot.entry = Some(entry);
    }

    fn stored_from(
        &self,
        server: &McpServer,
        snapshot: &ServerCatalogSnapshot,
        fetched_at: DateTime<Utc>,
    ) -> StoredCatalog {
        StoredCatalog {
            updated_at: server.updated_at,
            fetched_at: Some(fetched_at),
            content_hash: Some(content_fingerprint(snapshot)),
            // A publication identity is regenerated on every publication, even
            // when the content is unchanged, so followers can prove progress.
            publication_id: Some(Uuid::new_v4().to_string()),
            snapshot: snapshot.clone(),
        }
    }

    /// Unconditional write of the stored envelope to the shared backend.
    async fn write_shared(&self, server_id: Uuid, stored: &StoredCatalog) {
        match &self.backend {
            CacheBackend::Local => {}
            CacheBackend::Sqlite(store) => {
                let payload = match serde_json::to_string(stored) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!(error = %err, server_id = %server_id, "failed to serialize MCP catalog for SQLite");
                        return;
                    }
                };
                if let Err(err) = store
                    .coordinator
                    .put(
                        CATALOG_NAMESPACE,
                        &server_id.to_string(),
                        &payload,
                        store.ttl_seconds,
                    )
                    .await
                {
                    warn!(error = %err, server_id = %server_id, "failed to write MCP catalog to SQLite");
                }
            }
            CacheBackend::Valkey {
                manager,
                ttl_seconds,
            } => {
                let payload = match serde_json::to_string(stored) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!(error = %err, server_id = %server_id, "failed to serialize MCP catalog for valkey");
                        return;
                    }
                };
                let mut manager = manager.clone();
                // The retention window is independent of the serving-age
                // policy: the stored snapshot outlives the moment it may be
                // served.
                if let Err(err) = manager
                    .set_ex::<_, _, ()>(catalog_key(server_id), payload, *ttl_seconds)
                    .await
                {
                    warn!(error = %err, server_id = %server_id, "failed to write MCP catalog to valkey");
                }
            }
        }
    }
}

fn stored_from_entry(entry: CacheEntry) -> StoredCatalog {
    StoredCatalog {
        updated_at: entry.updated_at,
        fetched_at: entry.fetched_at,
        content_hash: entry.content_hash,
        publication_id: entry.publication_id,
        snapshot: entry.snapshot,
    }
}

fn stored_from_stored(stored: &StoredCatalog) -> CacheEntry {
    CacheEntry {
        updated_at: stored.updated_at,
        fetched_at: stored.fetched_at,
        content_hash: stored.content_hash.clone(),
        publication_id: stored.publication_id.clone(),
        snapshot: stored.snapshot.clone(),
    }
}

/// A revisioned marker that represents "no catalog" in a shared backend.
///
/// It is written instead of deleting the key, so an in-flight publisher that
/// captured the previous payload fails its compare-and-swap.
fn tombstone_payload() -> String {
    format!("{{\"tombstone\":true,\"revision\":\"{}\"}}", Uuid::new_v4())
}

fn is_tombstone(payload: &str) -> bool {
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| value.get("tombstone").and_then(Value::as_bool))
        .unwrap_or(false)
}

async fn valkey_baseline(
    manager: &ConnectionManager,
    server_id: Uuid,
    ttl_seconds: u64,
) -> anyhow::Result<String> {
    let mut manager = manager.clone();
    let payload: String = redis::Script::new(VALKEY_BASELINE_SCRIPT)
        .key(catalog_key(server_id))
        .arg(tombstone_payload())
        .arg(ttl_seconds.max(1))
        .invoke_async(&mut manager)
        .await?;
    Ok(payload)
}

async fn valkey_publish(
    manager: &ConnectionManager,
    server_id: Uuid,
    expected: &str,
    payload: &str,
    ttl_seconds: u64,
) -> anyhow::Result<bool> {
    let mut manager = manager.clone();
    let replaced: i64 = redis::Script::new(VALKEY_PUBLISH_SCRIPT)
        .key(catalog_key(server_id))
        .arg(expected)
        .arg(payload)
        .arg(ttl_seconds.max(1))
        .invoke_async(&mut manager)
        .await?;
    Ok(replaced == 1)
}

fn catalog_key(server_id: Uuid) -> String {
    format!("{MCP_CATALOG_VALKEY_KEY_PREFIX}{server_id}")
}

#[cfg(test)]
mod tests;
