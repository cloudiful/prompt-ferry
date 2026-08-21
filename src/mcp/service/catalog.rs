use std::sync::Arc;

use futures::{StreamExt, stream};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{db, mcp::cache::McpCatalogCache};

use super::snapshot::fetch_server_snapshot_with_storage;

const WARMUP_CONCURRENCY: usize = 8;

#[derive(Clone)]
pub struct McpCatalogService {
    inner: Arc<McpCatalogServiceInner>,
}

struct McpCatalogServiceInner {
    repository: crate::db::ConfigRepository,
    cache: McpCatalogCache,
}

impl McpCatalogService {
    pub fn new(pool: sqlx::PgPool, cache: McpCatalogCache) -> Self {
        Self::new_with_repository(crate::db::ConfigRepository::postgres(&pool), cache)
    }

    pub fn new_with_repository(
        repository: crate::db::ConfigRepository,
        cache: McpCatalogCache,
    ) -> Self {
        Self {
            inner: Arc::new(McpCatalogServiceInner { repository, cache }),
        }
    }

    pub fn spawn_refresh(&self, server: db::McpServer) {
        let service = self.clone();
        tokio::spawn(async move {
            service.refresh_in_background(server).await;
        });
    }

    pub async fn warm_enabled_servers(&self) {
        let servers = match self.inner.repository.list_all_mcp_servers().await {
            Ok(servers) => servers,
            Err(err) => {
                warn!(error = %err, "failed to list MCP servers during startup warmup");
                return;
            }
        };
        let enabled = servers.into_iter().filter(|server| server.enabled);
        stream::iter(enabled)
            .for_each_concurrent(WARMUP_CONCURRENCY, |server| async move {
                self.refresh_in_background(server).await;
            })
            .await;
        info!(
            category = "mcp_catalog_warmup",
            "MCP catalog startup warmup completed"
        );
    }

    pub async fn refresh_server_by_id(
        &self,
        server_id: Uuid,
    ) -> anyhow::Result<(db::McpServer, super::ServerCatalogSnapshot)> {
        let server = self
            .inner
            .repository
            .get_mcp_server(server_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("mcp server not found"))?;
        if !server.enabled {
            self.inner.cache.invalidate(server_id).await;
            return Err(anyhow::anyhow!("mcp server is disabled"));
        }
        let snapshot = self.refresh_server(&server).await?;
        Ok((server, snapshot))
    }

    pub async fn refresh_server(
        &self,
        server: &db::McpServer,
    ) -> anyhow::Result<super::ServerCatalogSnapshot> {
        let storage = crate::mcp::McpRuntimeStorage::from_repository(self.inner.repository.clone());
        let snapshot = fetch_server_snapshot_with_storage(Some(&storage), server)
            .await
            .map_err(|err| {
                anyhow::anyhow!("failed to refresh mcp catalog for '{}': {err}", server.name)
            })?;
        self.inner.cache.put(server, snapshot.clone()).await;
        info!(
            category = "mcp_catalog_cache",
            server_name = %server.name,
            "updated MCP catalog cache from upstream"
        );
        Ok(snapshot)
    }

    pub async fn invalidate(&self, server_id: Uuid) {
        self.inner.cache.invalidate(server_id).await;
    }

    async fn refresh_in_background(&self, server: db::McpServer) {
        if !self
            .inner
            .cache
            .mark_refresh_inflight(server.server_id)
            .await
        {
            info!(
                category = "mcp_catalog_warmup",
                server_id = %server.server_id,
                "MCP catalog refresh already in flight"
            );
            return;
        }

        let result = self.refresh_server(&server).await;
        self.inner.cache.finish_refresh(server.server_id).await;
        if let Err(err) = result {
            warn!(
                category = "mcp_catalog_warmup",
                server_id = %server.server_id,
                server_name = %server.name,
                error = %err,
                "MCP catalog refresh failed; existing cache retained"
            );
        }
    }
}
