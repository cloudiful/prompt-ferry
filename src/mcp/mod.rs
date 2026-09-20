mod aggregate;
mod builtin;
mod cache;
mod entry;
mod filtering;
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
pub(crate) use entry::handle_stream_with_storage;
pub use entry::{
    McpRequestContext, McpTransportResponse, handle, handle_stream,
    handle_stream_with_session_store, inspect_server,
};
pub use provider_usage::{FirecrawlBalanceClient, ProviderBalance, ProviderBalanceError};
// MCP provider preset contract (issue #296 Phase 1): re-exported here so
// transport/runtime code resolves provider metadata from the mcp module.
pub use crate::db::{
    MCP_PROVIDER_CONTEXT7, MCP_PROVIDER_FIRECRAWL, MCP_PROVIDER_GENERIC, MCP_PROVIDER_MINIMAX,
    MCP_PROVIDER_REGISTRY, McpProviderAuth, McpProviderInfo, McpProviderUnit,
    effective_provider_kind, is_known_mcp_provider, mcp_provider_info,
};
pub use service::{McpCatalogService, catalog_for_server};
pub use session_store::McpSessionStore;
pub(crate) use transport::with_tracked_credits;

/// Storage selected by the worker runtime for MCP configuration lookups.
///
#[derive(Clone)]
pub(crate) struct McpRuntimeStorage {
    repository: crate::db::ConfigRepository,
}

impl McpRuntimeStorage {
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            repository: crate::db::ConfigRepository::postgres(&pool),
        }
    }

    pub(crate) fn from_repository(repository: crate::db::ConfigRepository) -> Self {
        Self { repository }
    }

    pub(crate) fn repository(&self) -> &crate::db::ConfigRepository {
        &self.repository
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
        Self {
            storage: McpRuntimeStorage::from_repository(state.config_repository.clone()),
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
    ) -> Self {
        Self {
            storage,
            catalog_cache: McpCatalogCache::from_config_with_sqlite(
                config,
                Some(sqlite_pool.clone()),
            )
            .await,
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
