mod aggregate;
mod builtin;
mod cache;
pub(crate) mod call_record;
mod entry;
mod filtering;
mod freshness;
mod output_schema;
mod protocol;
mod provider_usage;
mod routing;
mod server;
mod service;
mod session_store;
pub(crate) mod targeting;
mod transport;

pub use cache::{MCP_CATALOG_VALKEY_KEY_PREFIX, McpCatalogCache, ServerCatalogSnapshot};
pub(crate) use call_record::McpCallTracker;
pub(crate) use entry::handle_stream_with_storage;
pub use entry::{
    McpRequestContext, McpTransportResponse, handle, handle_stream,
    handle_stream_with_session_store, inspect_server,
};
pub use freshness::{CatalogFreshness, CatalogFreshnessPolicy};
pub use provider_usage::{FirecrawlBalanceClient, ProviderBalance, ProviderBalanceError};
// MCP provider preset contract (issue #296 Phase 1): re-exported here so
// transport/runtime code resolves provider metadata from the mcp module.
pub use crate::db::{
    MCP_PROVIDER_CONTEXT7, MCP_PROVIDER_FIRECRAWL, MCP_PROVIDER_GENERIC, MCP_PROVIDER_MINIMAX,
    MCP_PROVIDER_REGISTRY, McpProviderAuth, McpProviderInfo, McpProviderUnit,
    effective_provider_kind, is_known_mcp_provider, mcp_provider_info,
};
pub(crate) use builtin::MiniMaxTransport;
#[cfg(test)]
pub(crate) use builtin::{MiniMaxRequest, MiniMaxResponse};
pub use service::{
    AcquiredCatalog, CATALOG_REFRESH_DRAIN_TIMEOUT, CatalogRefreshPolicy, McpCatalogService,
};
pub use session_store::McpSessionStore;
pub(crate) use transport::with_tracked_credits;

/// Storage selected by the worker runtime for MCP configuration lookups.
///
/// The storage also carries the tracker of the request it belongs to, so the
/// key selection and the tool outcome that happen inside the rmcp session
/// task reach the settlement of the same request (issue #1179 P2).
#[derive(Clone)]
pub(crate) struct McpRuntimeStorage {
    repository: crate::db::ConfigRepository,
    tracker: McpCallTracker,
    /// The shared quota cache of the worker. It is a clone of the worker
    /// cache, so the cached snapshots, the feedback store and the
    /// reservations are the same shared state the AI path reads and writes
    /// (issue #1179 P1). A runtime without an admin quota context has none.
    quota: Option<crate::worker_admin::token_plan_cache::TokenPlanQuotaCache>,
    /// The builtin provider transport. `None` uses the proxied HTTP client;
    /// the in-process tests install a stub instead.
    builtin_transport: Option<std::sync::Arc<dyn MiniMaxTransport>>,
}

impl McpRuntimeStorage {
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            repository: crate::db::ConfigRepository::postgres(&pool),
            tracker: McpCallTracker::new(),
            quota: None,
            builtin_transport: None,
        }
    }

    pub(crate) fn from_repository(repository: crate::db::ConfigRepository) -> Self {
        Self {
            repository,
            tracker: McpCallTracker::new(),
            quota: None,
            builtin_transport: None,
        }
    }

    pub(crate) fn repository(&self) -> &crate::db::ConfigRepository {
        &self.repository
    }

    /// The tracker of the request this storage serves.
    pub(crate) fn tracker(&self) -> &McpCallTracker {
        &self.tracker
    }

    /// The shared quota cache, when the runtime published one.
    pub(crate) fn quota(
        &self,
    ) -> Option<&crate::worker_admin::token_plan_cache::TokenPlanQuotaCache> {
        self.quota.as_ref()
    }

    /// A storage that reads the shared quota cache.
    pub(crate) fn with_quota_cache(
        &self,
        cache: crate::worker_admin::token_plan_cache::TokenPlanQuotaCache,
    ) -> Self {
        Self {
            quota: Some(cache),
            ..self.clone()
        }
    }

    /// A storage that tracks one request.
    pub(crate) fn with_tracker(&self, tracker: McpCallTracker) -> Self {
        Self {
            tracker,
            ..self.clone()
        }
    }

    /// A storage that drives the builtin provider through the given transport.
    #[cfg(test)]
    pub(crate) fn with_builtin_transport(
        &self,
        transport: std::sync::Arc<dyn MiniMaxTransport>,
    ) -> Self {
        Self {
            builtin_transport: Some(transport),
            ..self.clone()
        }
    }

    /// The builtin provider transport, when one is installed.
    pub(crate) fn builtin_transport(&self) -> Option<&std::sync::Arc<dyn MiniMaxTransport>> {
        self.builtin_transport.as_ref()
    }
}

#[derive(Clone)]
pub(crate) struct McpRuntimeState {
    pub(crate) storage: McpRuntimeStorage,
    pub(crate) catalog_cache: McpCatalogCache,
    pub(crate) session_store:
        Option<std::sync::Arc<dyn rmcp::transport::streamable_http_server::session::SessionStore>>,
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) request_content_logging: std::sync::Arc<
        tokio::sync::RwLock<crate::worker_admin_types::RequestContentLoggingResponse>,
    >,
}

impl McpRuntimeState {
    pub(crate) fn from_admin_state(state: &crate::worker_admin::AdminState) -> Self {
        // Issue #1179 P1: the builtin providers reuse the worker quota cache,
        // so the same snapshot and feedback state backs the AI routes and the
        // builtin MCP tools.
        let storage = McpRuntimeStorage::from_repository(state.config_repository.clone())
            .with_quota_cache(state.token_plan_quota.clone());
        Self {
            storage,
            catalog_cache: state.mcp_catalog_cache.clone(),
            session_store: state.mcp_session_store.clone(),
            allowed_origins: state.mcp_allowed_origins.clone(),
            request_content_logging: state.request_content_logging.clone(),
        }
    }

    pub(crate) async fn sqlite(
        config: &crate::config::WorkerConfig,
        storage: McpRuntimeStorage,
        sqlite_pool: sqlx::SqlitePool,
        catalog_cache: McpCatalogCache,
    ) -> Self {
        Self {
            storage,
            catalog_cache,
            session_store: McpSessionStore::from_config_with_sqlite(config, Some(sqlite_pool))
                .await,
            allowed_origins: config.mcp_allowed_origins.clone(),
            request_content_logging: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::worker_admin_types::RequestContentLoggingResponse {
                    mode: crate::worker_admin_types::RequestContentLoggingMode::Off,
                    raw_retention_days: 3,
                },
            )),
        }
    }
}

/// Maximum size of an MCP request body (relay streaming, worker chunk
/// assembly, and the rmcp server config all enforce this same bound).
pub const MAX_MCP_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
