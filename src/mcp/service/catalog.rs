//! The coordinated MCP catalog service.
//!
//! One entrypoint owns every catalog refresh: the manual refresh, the catalog
//! reads, the startup warmup and the periodic pass all go through it. The
//! bounded, cancellable ownership state lives in the cache's refresh
//! coordinator, so every service wrapper sharing one cache shares the same
//! permit pool, per-server scheduling and cancellation scope.

use std::{sync::Arc, time::Duration};

use chrono::Utc;
use futures::{StreamExt, stream};
use tokio::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    db,
    mcp::{
        cache::{
            CatalogActivity, CatalogRefreshCoordinator, McpCatalogCache, RefreshReservation,
            ServerCatalogSnapshot,
        },
        freshness::CatalogFreshness,
    },
};

use super::snapshot::fetch_server_snapshot_with_storage;

/// The production bound on concurrent catalog refresh work.
const CATALOG_REFRESH_CONCURRENCY: usize = 8;

/// How long the managed worker waits for cancelled refresh work to stop.
pub const CATALOG_REFRESH_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// The bounds of one catalog refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogRefreshPolicy {
    /// The overall ceiling of one catalog acquisition, including the wait for
    /// an owner permit, the lease, the upstream fetch and the publication.
    pub service_ceiling: Duration,
    /// How often a follower re-reads the cache while it waits for the refresh
    /// another worker owns.
    pub follower_poll_interval: Duration,
    /// The safety margin between the refresh lease lifetime and the longest
    /// work the lease has to cover.
    pub lease_margin: Duration,
    /// How long a failed background refresh suppresses the next background
    /// attempt for the same server configuration generation.
    pub failure_backoff: Duration,
    /// How many servers are refreshed at the same time.
    pub concurrency: usize,
}

impl CatalogRefreshPolicy {
    pub fn production() -> Self {
        Self {
            service_ceiling: Duration::from_secs(60),
            follower_poll_interval: Duration::from_millis(100),
            lease_margin: Duration::from_secs(30),
            failure_backoff: Duration::from_secs(30),
            concurrency: CATALOG_REFRESH_CONCURRENCY,
        }
    }
}

impl Default for CatalogRefreshPolicy {
    fn default() -> Self {
        Self::production()
    }
}

/// One public catalog operation: its absolute deadline and tracked activity.
pub(super) struct CatalogOperation {
    pub(super) activity: CatalogActivity,
    pub(super) deadline: Instant,
}

/// One acquired catalog and how it was obtained.
#[derive(Clone, Debug)]
pub struct AcquiredCatalog {
    pub snapshot: ServerCatalogSnapshot,
    pub freshness: CatalogFreshness,
    /// Whether this call fetched the snapshot itself.
    pub refreshed: bool,
}

/// Whether a background catalog task may touch this server.
///
/// Personal OAuth servers are resolved on demand for their owner only; a
/// background pass has no owner and must never borrow one.
fn background_catalog_refresh_allowed(server: &db::McpServer) -> bool {
    server.effective_auth_mode() != crate::db::MCP_AUTH_MODE_OAUTH
}

#[derive(Clone)]
pub struct McpCatalogService {
    inner: Arc<McpCatalogServiceInner>,
}

struct McpCatalogServiceInner {
    repository: crate::db::ConfigRepository,
    cache: McpCatalogCache,
    policy: CatalogRefreshPolicy,
    /// Personal MCP OAuth backend. Refreshes triggered by the owner need it to
    /// resolve that owner's token; background refreshes skip such servers.
    oauth: Option<std::sync::Arc<crate::mcp::oauth::McpOAuthService>>,
}

impl McpCatalogService {
    pub fn new(pool: sqlx::PgPool, cache: McpCatalogCache) -> Self {
        Self::new_with_repository(crate::db::ConfigRepository::postgres(&pool), cache)
    }

    pub fn new_with_repository(
        repository: crate::db::ConfigRepository,
        cache: McpCatalogCache,
    ) -> Self {
        let policy = CatalogRefreshPolicy::production();
        cache.configure_refresh_concurrency(policy.concurrency);
        Self {
            inner: Arc::new(McpCatalogServiceInner {
                repository,
                cache,
                policy,
                oauth: None,
            }),
        }
    }

    /// A service that reads and refreshes through the same storage the
    /// runtime uses.
    pub(crate) fn from_storage(
        storage: &crate::mcp::McpRuntimeStorage,
        cache: &McpCatalogCache,
    ) -> Self {
        let service = Self::new_with_repository(storage.repository().clone(), cache.clone());
        match storage.oauth() {
            Some(oauth) => service.with_oauth_service(oauth.clone()),
            None => service,
        }
    }

    /// A service that can resolve personal MCP OAuth credentials.
    pub(crate) fn with_oauth_service(
        mut self,
        oauth: std::sync::Arc<crate::mcp::oauth::McpOAuthService>,
    ) -> Self {
        self.inner = Arc::new(McpCatalogServiceInner {
            repository: self.inner.repository.clone(),
            cache: self.inner.cache.clone(),
            policy: self.inner.policy,
            oauth: Some(oauth),
        });
        self
    }

    /// Replace the refresh bounds.
    ///
    /// The refresh coordinator belongs to the cache and is shared by every
    /// wrapper, so this never replaces it; it only adopts the bound before any
    /// work starts. Production uses [`CatalogRefreshPolicy::production`].
    pub fn with_refresh_policy(mut self, policy: CatalogRefreshPolicy) -> Self {
        self.inner
            .cache
            .configure_refresh_concurrency(policy.concurrency);
        self.inner = Arc::new(McpCatalogServiceInner {
            repository: self.inner.repository.clone(),
            cache: self.inner.cache.clone(),
            policy,
            oauth: self.inner.oauth.clone(),
        });
        self
    }

    pub fn refresh_policy(&self) -> CatalogRefreshPolicy {
        self.inner.policy
    }

    pub(super) fn cache(&self) -> &McpCatalogCache {
        &self.inner.cache
    }

    pub(super) fn coordinator(&self) -> Arc<CatalogRefreshCoordinator> {
        self.inner.cache.refresh_coordinator().clone()
    }

    /// Begin one public catalog operation.
    ///
    /// The absolute deadline and the tracked activity are created here, before
    /// the caller's first await, and are then threaded through every stage of
    /// that operation. Public helpers that do more than one lookup must reuse
    /// this context instead of starting a second one.
    pub(super) fn begin_operation(&self) -> Option<CatalogOperation> {
        let activity = self.coordinator().admit()?;
        Some(CatalogOperation {
            activity,
            deadline: self.operation_deadline(),
        })
    }

    /// The one coordinated acquisition entrypoint of every catalog reader.
    ///
    /// The whole operation runs under one absolute deadline and one tracked
    /// activity: the cache lookup, the owner permit, the lease, the repository
    /// check, the upstream fetch and the publication all share the ceiling.
    /// Cancellation is observed at every stage, including an in-progress cache
    /// or repository wait.
    pub async fn acquire(&self, server: &db::McpServer) -> anyhow::Result<AcquiredCatalog> {
        let Some(operation) = self.begin_operation() else {
            return Err(operation_cancelled(server));
        };
        self.acquire_with_operation(server, &operation)
            .await
            .map_err(|err| not_ready(server, err))
    }

    /// Acquire one catalog inside an already-started operation.
    ///
    /// The caller owns the single deadline and activity for the whole helper;
    /// this never starts a second context.
    pub(super) async fn acquire_with_operation(
        &self,
        server: &db::McpServer,
        operation: &CatalogOperation,
    ) -> anyhow::Result<AcquiredCatalog> {
        self.acquire_until(server, operation.deadline, &operation.activity)
            .await
    }

    /// Refresh one server by id and return the stored snapshot.
    ///
    /// The repository read that loads the row is part of the same bounded,
    /// tracked operation as the refresh itself.
    pub async fn refresh_server_by_id(
        &self,
        server_id: Uuid,
    ) -> anyhow::Result<(db::McpServer, ServerCatalogSnapshot)> {
        let Some(operation) = self.begin_operation() else {
            return Err(anyhow::anyhow!("the MCP catalog refresh was cancelled"));
        };
        let deadline = operation.deadline;
        let server = self
            .bounded(deadline, self.inner.repository.get_mcp_server(server_id))
            .await?
            .map_err(|err| anyhow::anyhow!("failed to load the mcp server: {err}"))?
            .ok_or_else(|| anyhow::anyhow!("mcp server not found"))?;
        if !server.enabled {
            // The invalidation is part of the same operation: a blocked or slow
            // shared store must not outlive the deadline or the cancellation.
            self.bounded(deadline, self.inner.cache.invalidate(server_id))
                .await?;
            return Err(anyhow::anyhow!("mcp server is disabled"));
        }
        let snapshot = self
            .force_refresh(&server, deadline, &operation.activity)
            .await?;
        Ok((server, snapshot))
    }

    /// Refresh one server regardless of the cached age.
    pub async fn refresh_server(
        &self,
        server: &db::McpServer,
    ) -> anyhow::Result<ServerCatalogSnapshot> {
        let Some(operation) = self.begin_operation() else {
            return Err(operation_cancelled(server));
        };
        self.force_refresh(server, operation.deadline, &operation.activity)
            .await
            .map_err(|err| not_ready(server, err))
    }

    pub async fn invalidate(&self, server_id: Uuid) {
        self.inner.cache.invalidate(server_id).await;
    }

    /// Refresh one server in the background through the shared coordinator.
    ///
    /// A personal OAuth server is never refreshed by a background task: its
    /// catalog needs the owner's token, so the owner triggers it explicitly.
    pub fn spawn_refresh(&self, server: db::McpServer) {
        if !background_catalog_refresh_allowed(&server) {
            return;
        }
        self.spawn_reserved_refresh(server);
    }

    /// Stop scheduling refresh work and wait for the owned refreshes to stop.
    pub fn cancel_refreshes(&self) {
        self.coordinator().cancel();
    }

    /// Wait until every tracked operation, including direct reads and manual
    /// refreshes, finished or exhausted the drain budget.
    pub async fn drain_refreshes(&self, timeout: Duration) {
        self.coordinator().cancel_and_drain(timeout).await;
    }

    /// Load the catalog of every enabled server.
    ///
    /// A server whose catalog is already fresh is not fetched again; a server
    /// without a catalog is loaded now, and an aged catalog is refreshed
    /// through the shared coordinator.
    pub async fn warm_enabled_servers(&self) {
        let Some(activity) = self.coordinator().admit() else {
            return;
        };
        let deadline = self.operation_deadline();
        let servers = match self
            .bounded(deadline, self.inner.repository.list_all_mcp_servers())
            .await
        {
            Ok(Ok(servers)) => servers,
            Ok(Err(err)) => {
                warn!(error = %err, "failed to list MCP servers during startup warmup");
                return;
            }
            Err(err) => {
                warn!(error = %err, "the startup MCP server listing exceeded its deadline or was cancelled");
                return;
            }
        };
        drop(activity);
        let enabled = servers
            .into_iter()
            .filter(|server| server.enabled)
            .filter(background_catalog_refresh_allowed);
        stream::iter(enabled)
            .for_each_concurrent(self.inner.policy.concurrency, |server| async move {
                if let Err(err) = self.acquire(&server).await {
                    warn!(
                        category = "mcp_catalog_warmup",
                        server_name = %server.name,
                        error = %err,
                        "MCP catalog warmup failed for one server"
                    );
                }
            })
            .await;
        info!(
            category = "mcp_catalog_warmup",
            "MCP catalog startup warmup completed"
        );
    }

    /// Refresh every cached catalog that is no longer fresh.
    ///
    /// A catalog that was never fetched stays cold: the read that needs it
    /// loads it, so the periodic pass never turns into a bulk cold start.
    pub async fn refresh_aging_entries(&self) {
        let Some(activity) = self.coordinator().admit() else {
            return;
        };
        let deadline = self.operation_deadline();
        let servers = match self
            .bounded(deadline, self.inner.repository.list_all_mcp_servers())
            .await
        {
            Ok(Ok(servers)) => servers,
            Ok(Err(err)) => {
                warn!(error = %err, "failed to list MCP servers during the periodic catalog refresh");
                return;
            }
            Err(err) => {
                warn!(error = %err, "the periodic MCP server listing exceeded its deadline or was cancelled");
                return;
            }
        };
        drop(activity);
        let enabled = servers
            .into_iter()
            .filter(|server| server.enabled)
            .filter(background_catalog_refresh_allowed);
        stream::iter(enabled)
            .for_each_concurrent(self.inner.policy.concurrency, |server| async move {
                let Some(activity) = self.coordinator().admit() else {
                    return;
                };
                let deadline = self.operation_deadline();
                let lookup = match self
                    .bounded(deadline, self.inner.cache.lookup(&server))
                    .await
                {
                    Ok(lookup) => lookup,
                    Err(_) => return,
                };
                if lookup.freshness == CatalogFreshness::Fresh
                    || lookup.freshness == CatalogFreshness::Missing
                {
                    return;
                }
                self.refresh_reserved(&server, deadline, &activity).await;
            })
            .await;
    }

    /// The absolute deadline of one operation.
    pub(super) fn operation_deadline(&self) -> Instant {
        Instant::now() + self.inner.policy.service_ceiling
    }

    /// Await one stage under the operation deadline and shared cancellation.
    ///
    /// Every asynchronous step goes through this helper, so a blocked cache,
    /// repository, permit, lease or publish wait terminates with the operation
    /// instead of outliving it. The deadline is absolute, never restarted.
    pub(super) async fn bounded<F, T>(&self, deadline: Instant, future: F) -> anyhow::Result<T>
    where
        F: std::future::Future<Output = T>,
    {
        let coordinator = self.coordinator();
        let cancelled = coordinator.cancelled();
        tokio::pin!(cancelled);
        tokio::select! {
            _ = &mut cancelled => Err(anyhow::anyhow!("the MCP catalog operation was cancelled")),
            result = tokio::time::timeout_at(deadline, future) => result
                .map_err(|_| anyhow::anyhow!("the MCP catalog operation exceeded its deadline")),
        }
    }

    async fn acquire_until(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<AcquiredCatalog> {
        let lookup = self
            .bounded(deadline, self.inner.cache.lookup(server))
            .await?;
        match (lookup.freshness, lookup.snapshot) {
            (CatalogFreshness::Fresh, Some(snapshot)) => Ok(AcquiredCatalog {
                snapshot,
                freshness: CatalogFreshness::Fresh,
                refreshed: false,
            }),
            (CatalogFreshness::Stale, Some(snapshot)) => {
                self.schedule_refresh(server);
                Ok(AcquiredCatalog {
                    snapshot,
                    freshness: CatalogFreshness::Stale,
                    refreshed: false,
                })
            }
            _ => self.recover(server, deadline, activity).await,
        }
    }

    async fn recover(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<AcquiredCatalog> {
        self.refresh_coordinated(server, deadline, activity).await
    }

    /// Recover an expired or missing catalog under one owner permit.
    async fn refresh_coordinated(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<AcquiredCatalog> {
        let coordinator = self.coordinator();
        let Some(permit) = self.bounded(deadline, coordinator.acquire_permit()).await? else {
            return Err(anyhow::anyhow!("the MCP catalog refresh was cancelled"));
        };
        let lease_ttl = self.lease_ttl();
        let lease = self
            .bounded(
                deadline,
                self.inner.cache.try_acquire_tracked_refresh_lease(
                    server.server_id,
                    lease_ttl,
                    self.release_budget(),
                    activity.clone(),
                ),
            )
            .await?;
        let Some(lease) = lease else {
            drop(permit);
            return self.await_owner_refresh(server, deadline).await;
        };
        let outcome = self.owner_work(server, deadline).await;
        // Explicit release inside the reserved safety margin: a shutdown never
        // depends on the lease expiry and never waits past the margin.
        self.release_bounded(lease).await;
        drop(permit);
        outcome
    }

    /// Reserve, run and release one background refresh inline.
    async fn refresh_reserved(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) {
        let Some(reservation) = self.reserve(server) else {
            return;
        };
        let outcome = self.background_work(server, deadline, activity).await;
        reservation.finish();
        if let Err(err) = outcome {
            warn!(
                category = "mcp_catalog_refresh",
                server_name = %server.name,
                error = %err,
                "MCP catalog background refresh failed; the existing snapshot is retained"
            );
        }
    }

    /// Reserve one refresh and run it as a coordinator-tracked task.
    fn spawn_reserved_refresh(&self, server: db::McpServer) {
        let Some(reservation) = self.reserve(&server) else {
            return;
        };
        let service = McpCatalogService::clone(self);
        tokio::spawn(async move {
            let deadline = service.operation_deadline();
            let activity = reservation.activity();
            let outcome = service.background_work(&server, deadline, &activity).await;
            reservation.finish();
            if let Err(err) = outcome {
                warn!(
                    category = "mcp_catalog_cache",
                    server_name = %server.name,
                    error = %err,
                    "MCP catalog refresh failed; the existing snapshot is retained"
                );
            }
        });
    }

    fn reserve(&self, server: &db::McpServer) -> Option<RefreshReservation> {
        self.coordinator()
            .reserve(server.server_id, server.updated_at)
    }

    /// One unit of bounded background refresh work: a permit, then one
    /// coordinated fetch of one server.
    async fn background_work(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<()> {
        let coordinator = self.coordinator();
        let Some(permit) = self.bounded(deadline, coordinator.acquire_permit()).await? else {
            return Ok(());
        };
        let outcome = self.refresh_in_background(server, deadline, activity).await;
        drop(permit);
        outcome
    }

    /// Refresh in the background without waiting for another worker's refresh.
    ///
    /// A recent failure suppresses the next background attempt for the same
    /// server generation for a bounded window; a read that has to recover an
    /// expired catalog ignores that backoff.
    async fn refresh_in_background(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<()> {
        if self.coordinator().is_cancelled() {
            return Ok(());
        }
        if self
            .inner
            .cache
            .refresh_backoff_active(server.server_id, server.updated_at)
            .await
        {
            return Ok(());
        }
        let lease = self
            .bounded(
                deadline,
                self.inner.cache.try_acquire_tracked_refresh_lease(
                    server.server_id,
                    self.lease_ttl(),
                    self.release_budget(),
                    activity.clone(),
                ),
            )
            .await?;
        let Some(lease) = lease else {
            return Ok(());
        };
        let outcome = self.owner_work(server, deadline).await.map(|_| ());
        self.release_bounded(lease).await;
        match &outcome {
            Ok(()) => {
                self.inner
                    .cache
                    .clear_refresh_failure(server.server_id)
                    .await;
            }
            Err(_) => {
                self.inner
                    .cache
                    .record_refresh_failure(
                        server.server_id,
                        server.updated_at,
                        self.inner.policy.failure_backoff,
                    )
                    .await;
            }
        }
        outcome
    }

    /// Fetch and publish one catalog as the refresh owner.
    async fn owner_work(
        &self,
        server: &db::McpServer,
        deadline: Instant,
    ) -> anyhow::Result<AcquiredCatalog> {
        // Another worker may have published a fresh snapshot while this worker
        // waited for the lease; re-read before spending an upstream round trip.
        let current = self
            .bounded(deadline, self.inner.cache.lookup(server))
            .await?;
        if current.freshness == CatalogFreshness::Fresh
            && let Some(snapshot) = current.snapshot
        {
            return Ok(AcquiredCatalog {
                snapshot,
                freshness: CatalogFreshness::Fresh,
                refreshed: false,
            });
        }
        let baseline = self
            .bounded(deadline, self.inner.cache.baseline(server))
            .await??;
        let snapshot = Box::pin(self.fetch(server, deadline)).await?;
        self.publish(server, snapshot, &baseline, deadline).await
    }

    /// Wait for the refresh another worker owns.
    async fn await_owner_refresh(
        &self,
        server: &db::McpServer,
        deadline: Instant,
    ) -> anyhow::Result<AcquiredCatalog> {
        loop {
            let lookup = self
                .bounded(deadline, self.inner.cache.lookup(server))
                .await?;
            match (lookup.freshness, lookup.snapshot) {
                (CatalogFreshness::Fresh, Some(snapshot)) => {
                    return Ok(AcquiredCatalog {
                        snapshot,
                        freshness: CatalogFreshness::Fresh,
                        refreshed: false,
                    });
                }
                (CatalogFreshness::Stale, Some(snapshot)) => {
                    return Ok(AcquiredCatalog {
                        snapshot,
                        freshness: CatalogFreshness::Stale,
                        refreshed: false,
                    });
                }
                _ => {}
            }
            self.bounded(
                deadline,
                tokio::time::sleep(self.inner.policy.follower_poll_interval),
            )
            .await?;
        }
    }

    /// Wait for a publication produced after `baseline` was observed.
    ///
    /// A forced refresh must never report a snapshot that existed before the
    /// attempt: age alone cannot prove that the owner that was running
    /// actually published anything.
    async fn await_publication(
        &self,
        server: &db::McpServer,
        baseline: Option<String>,
        deadline: Instant,
    ) -> anyhow::Result<ServerCatalogSnapshot> {
        loop {
            let lookup = self
                .bounded(deadline, self.inner.cache.lookup(server))
                .await?;
            let published_since_baseline = lookup
                .publication_id
                .as_deref()
                .is_some_and(|publication_id| Some(publication_id) != baseline.as_deref());
            if published_since_baseline
                && lookup.freshness.is_servable()
                && let Some(snapshot) = lookup.snapshot
            {
                return Ok(snapshot);
            }
            self.bounded(
                deadline,
                tokio::time::sleep(self.inner.policy.follower_poll_interval),
            )
            .await?;
        }
    }

    async fn force_refresh(
        &self,
        server: &db::McpServer,
        deadline: Instant,
        activity: &CatalogActivity,
    ) -> anyhow::Result<ServerCatalogSnapshot> {
        // The baseline is observed before ownership is attempted, so a forced
        // follower can distinguish a real publication from an older snapshot.
        let baseline = self
            .bounded(deadline, self.inner.cache.baseline(server))
            .await??;
        let baseline_publication = baseline.lookup.publication_id.clone();
        let coordinator = self.coordinator();
        let Some(permit) = self.bounded(deadline, coordinator.acquire_permit()).await? else {
            return Err(anyhow::anyhow!("the MCP catalog refresh was cancelled"));
        };
        let lease = self
            .bounded(
                deadline,
                self.inner.cache.try_acquire_tracked_refresh_lease(
                    server.server_id,
                    self.lease_ttl(),
                    self.release_budget(),
                    activity.clone(),
                ),
            )
            .await?;
        let Some(lease) = lease else {
            drop(permit);
            return self
                .await_publication(server, baseline_publication, deadline)
                .await;
        };
        let outcome = self.fetch_and_publish(server, deadline).await;
        self.release_bounded(lease).await;
        drop(permit);
        outcome.map(|acquired| acquired.snapshot)
    }

    async fn fetch(
        &self,
        server: &db::McpServer,
        deadline: Instant,
    ) -> anyhow::Result<ServerCatalogSnapshot> {
        let mut storage =
            crate::mcp::McpRuntimeStorage::from_repository(self.inner.repository.clone());
        if let Some(oauth) = self.inner.oauth.clone() {
            storage = storage.with_oauth_service(oauth);
        }
        self.bounded(
            deadline,
            Box::pin(fetch_server_snapshot_with_storage(Some(&storage), server)),
        )
        .await?
        .map_err(|err| {
            anyhow::anyhow!("failed to refresh mcp catalog for '{}': {err}", server.name)
        })
    }

    async fn fetch_and_publish(
        &self,
        server: &db::McpServer,
        deadline: Instant,
    ) -> anyhow::Result<AcquiredCatalog> {
        // The compare-and-swap baseline is captured after ownership is taken
        // and immediately before the fetch, so waiting for the permit or the
        // lease cannot make an otherwise valid publication fail.
        let baseline = self
            .bounded(deadline, self.inner.cache.baseline(server))
            .await??;
        let snapshot = Box::pin(self.fetch(server, deadline)).await?;
        self.publish(server, snapshot, &baseline, deadline).await
    }

    /// Store a fetched snapshot only while its baseline is still current.
    async fn publish(
        &self,
        server: &db::McpServer,
        snapshot: ServerCatalogSnapshot,
        baseline: &crate::mcp::cache::CatalogBaseline,
        deadline: Instant,
    ) -> anyhow::Result<AcquiredCatalog> {
        let current = self
            .bounded(
                deadline,
                self.inner.repository.get_mcp_server(server.server_id),
            )
            .await?
            .map_err(|err| anyhow::anyhow!("failed to re-read the mcp server: {err}"))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "mcp server '{}' was removed during the catalog refresh",
                    server.name
                )
            })?;
        if current.updated_at != server.updated_at {
            // The configuration changed while the upstream call was running.
            // Publishing the snapshot under the old generation would
            // resurrect a schema the server no longer declares.
            return Err(anyhow::anyhow!(
                "mcp server '{}' configuration changed during the catalog refresh",
                server.name
            ));
        }
        let published = self
            .bounded(
                deadline,
                self.inner.cache.publish_if_current(
                    server,
                    &baseline.token,
                    snapshot.clone(),
                    Utc::now(),
                ),
            )
            .await??;
        if !published {
            // The shared envelope changed after the baseline was captured: the
            // entry was invalidated, or another peer published first. Writing
            // now would resurrect a snapshot the operator dropped.
            return Err(anyhow::anyhow!(
                "mcp server '{}' catalog was invalidated during the refresh",
                server.name
            ));
        }
        self.inner
            .cache
            .clear_refresh_failure(server.server_id)
            .await;
        info!(
            category = "mcp_catalog_cache",
            server_name = %server.name,
            "updated MCP catalog cache from upstream"
        );
        Ok(AcquiredCatalog {
            snapshot,
            freshness: CatalogFreshness::Fresh,
            refreshed: true,
        })
    }

    /// Release the lease inside the reserved safety margin.
    ///
    /// The margin is part of the lease lifetime that the ceiling does not use,
    /// so a slow backend release can never outlive the lease it releases.
    async fn release_bounded(&self, lease: crate::mcp::cache::RefreshLease) {
        let budget = self.release_budget();
        let mut release = Box::pin(lease.release());
        if tokio::time::timeout(budget, &mut release).await.is_err() {
            warn!("the MCP catalog refresh lease release exceeded its safety margin");
        }
    }

    fn release_budget(&self) -> Duration {
        (self.inner.policy.lease_margin / 2).max(Duration::from_millis(1))
    }

    /// Schedule one bounded, coalesced background refresh.
    ///
    /// The personal-OAuth exclusion is enforced here so every current and
    /// future caller inherits it: a stale read of an owner-scoped OAuth catalog
    /// must never schedule work that has no owner to authenticate as.
    fn schedule_refresh(&self, server: &db::McpServer) {
        if !background_catalog_refresh_allowed(server) {
            return;
        }
        self.spawn_reserved_refresh(server.clone());
    }

    /// The refresh lease outlives every operation it covers plus the bounded
    /// release margin, so it never expires while a refresh is still running.
    fn lease_ttl(&self) -> Duration {
        self.inner.policy.service_ceiling + self.inner.policy.lease_margin
    }
}

fn operation_cancelled(server: &db::McpServer) -> anyhow::Error {
    not_ready(
        server,
        anyhow::anyhow!("the MCP catalog refresh was cancelled"),
    )
}

pub(super) fn not_ready(server: &db::McpServer, err: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("mcp catalog is not ready for '{}': {err:#}", server.name)
}
