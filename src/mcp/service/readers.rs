//! The catalog readers.
//!
//! Every reader acquires its catalogs through the coordinated service, so a
//! cold or old catalog is recovered instead of silently skipped.

use futures::{StreamExt, stream};
use serde_json::Value;
use tracing::warn;

use crate::{db::McpServer, worker_admin_types::McpCatalogResponse};

use crate::mcp::cache::ServerCatalogSnapshot;

use super::{
    builder::{
        AggregateKind, CatalogVisibility, aggregate_item, catalog_items_for_server,
        entries_for_kind,
    },
    catalog::McpCatalogService,
};

impl McpCatalogService {
    /// The admin and named-server catalog of one server.
    ///
    /// The admin catalog keeps every upstream item, including the ones the
    /// runtime filters out. The named server is acquired through the
    /// coordinated service, so a cold or old catalog is recovered; the other
    /// servers only contribute their cached names to the aggregate naming.
    pub async fn catalog_for_server(
        &self,
        servers: &[McpServer],
        server_name: &str,
    ) -> anyhow::Result<McpCatalogResponse> {
        let Some(named) = servers.iter().find(|server| server.name == server_name) else {
            return Err(anyhow::anyhow!("mcp server not found or disabled"));
        };
        // One operation context for the whole helper: the named acquisition and
        // every sibling lookup share its absolute deadline and activity.
        let Some(operation) = self.begin_operation() else {
            return Err(anyhow::anyhow!("the MCP catalog read was cancelled"));
        };
        let acquired = self
            .acquire_with_operation(named, &operation)
            .await
            .map_err(|err| super::catalog::not_ready(named, err))?;
        let deadline = operation.deadline;
        let mut loaded = Vec::with_capacity(servers.len());
        for server in servers {
            if server.server_id == named.server_id {
                loaded.push((server.clone(), acquired.snapshot.clone()));
                continue;
            }
            match self.bounded(deadline, self.cache().lookup(server)).await {
                Ok(lookup) => {
                    if let Some(snapshot) = lookup.snapshot {
                        loaded.push((server.clone(), snapshot));
                    }
                }
                Err(err) => warn!(
                    category = "mcp_catalog_cache",
                    server_name = %server.name,
                    error = %err,
                    "MCP sibling catalog lookup was bounded out; the server is missing from the admin catalog"
                ),
            }
        }
        drop(operation);
        let tools = entries_for_kind(&loaded, AggregateKind::Tools, CatalogVisibility::Admin);
        let resources =
            entries_for_kind(&loaded, AggregateKind::Resources, CatalogVisibility::Admin);
        let prompts = entries_for_kind(&loaded, AggregateKind::Prompts, CatalogVisibility::Admin);

        Ok(McpCatalogResponse {
            tools: catalog_items_for_server(&tools, server_name, AggregateKind::Tools),
            resources: catalog_items_for_server(&resources, server_name, AggregateKind::Resources),
            prompts: catalog_items_for_server(&prompts, server_name, AggregateKind::Prompts),
        })
    }

    pub async fn aggregate_tools(&self, servers: &[McpServer]) -> anyhow::Result<Vec<Value>> {
        self.aggregate_prefixed_items(servers, AggregateKind::Tools)
            .await
    }

    pub async fn aggregate_resources(&self, servers: &[McpServer]) -> anyhow::Result<Vec<Value>> {
        self.aggregate_prefixed_items(servers, AggregateKind::Resources)
            .await
    }

    pub async fn aggregate_resource_templates(
        &self,
        servers: &[McpServer],
    ) -> anyhow::Result<Vec<Value>> {
        self.aggregate_prefixed_items(servers, AggregateKind::Templates)
            .await
    }

    pub async fn aggregate_prompts(&self, servers: &[McpServer]) -> anyhow::Result<Vec<Value>> {
        self.aggregate_prefixed_items(servers, AggregateKind::Prompts)
            .await
    }

    async fn aggregate_prefixed_items(
        &self,
        servers: &[McpServer],
        kind: AggregateKind,
    ) -> anyhow::Result<Vec<Value>> {
        let loaded = self.acquire_all(servers).await;
        Ok(entries_for_kind(&loaded, kind, CatalogVisibility::Public)
            .into_iter()
            .map(|entry| aggregate_item(entry, kind))
            .collect())
    }

    /// Acquire the catalog of every server.
    ///
    /// A server whose catalog cannot be recovered is left out of the
    /// aggregate and reported, so a configured server is never dropped
    /// silently.
    async fn acquire_all(&self, servers: &[McpServer]) -> Vec<(McpServer, ServerCatalogSnapshot)> {
        let service = McpCatalogService::clone(self);
        let results = stream::iter(servers.iter().cloned())
            .map(|server| {
                let service = McpCatalogService::clone(&service);
                async move {
                    let acquired = service.acquire(&server).await;
                    (server, acquired)
                }
            })
            .buffer_unordered(self.refresh_policy().concurrency)
            .collect::<Vec<_>>()
            .await;
        let mut loaded = Vec::with_capacity(results.len());
        for (server, acquired) in results {
            match acquired {
                Ok(acquired) => loaded.push((server, acquired.snapshot)),
                Err(err) => warn!(
                    category = "mcp_catalog_cache",
                    server_name = %server.name,
                    error = %err,
                    "MCP server catalog is unavailable; the server is missing from the aggregate catalog"
                ),
            }
        }
        loaded
    }
}
