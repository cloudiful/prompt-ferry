mod aggregate;
mod cache;
mod entry;
mod filtering;
mod protocol;
mod quota;
mod routing;
mod server;
mod service;
mod session_store;
pub(crate) mod targeting;
mod transport;

pub use cache::{MCP_CATALOG_VALKEY_KEY_PREFIX, McpCatalogCache, ServerCatalogSnapshot};
pub use entry::{
    McpRequestContext, McpTransportResponse, handle, handle_stream,
    handle_stream_with_session_store, inspect_server,
};
pub use quota::{McpQuotaValkey, QuotaDecision, prepare_quota, record_credential_failure};
pub use service::{McpCatalogService, catalog_for_server};
pub use session_store::McpSessionStore;
pub(crate) use transport::{tracked_credits_used, tracked_upstream_failure, with_tracked_credits};

/// Maximum size of an MCP request body (relay streaming, worker chunk
/// assembly, and the rmcp server config all enforce this same bound).
pub const MAX_MCP_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
