use std::sync::Arc;

use redis::{AsyncCommands, aio::ConnectionManager};
use rmcp::transport::streamable_http_server::session::{
    SessionState, SessionStore, SessionStoreError,
};
use tracing::warn;

use crate::config::WorkerConfig;

pub const MCP_SESSION_VALKEY_KEY_PREFIX: &str = "pfy:mcp-session:";

#[derive(Clone)]
pub struct McpSessionStore {
    manager: ConnectionManager,
    ttl_seconds: u64,
}

impl McpSessionStore {
    pub async fn from_config(config: &WorkerConfig) -> Option<Arc<dyn SessionStore>> {
        let url = config.valkey_url.trim();
        if url.is_empty() {
            return None;
        }
        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to open valkey client for MCP sessions");
                return None;
            }
        };
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to connect valkey for MCP sessions");
                return None;
            }
        };
        Some(Arc::new(Self {
            manager,
            ttl_seconds: config.session_ttl_seconds,
        }))
    }
}

#[async_trait::async_trait]
impl SessionStore for McpSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        let key = mcp_session_cache_key(session_id);
        let mut manager = self.manager.clone();
        let value: Option<String> = match manager.get(&key).await {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, session_id, "failed to load MCP session from valkey");
                return Err(redis_session_error("load", err));
            }
        };
        let Some(value) = value else {
            return Ok(None);
        };
        let state = match serde_json::from_str(&value) {
            Ok(state) => state,
            Err(err) => {
                warn!(error = %err, session_id, "invalid MCP session valkey payload");
                return Err(redis_session_error("deserialize", err));
            }
        };
        let ttl = i64::try_from(self.ttl_seconds).unwrap_or(i64::MAX);
        if let Err(err) = manager.expire::<_, bool>(&key, ttl).await {
            // The session data was already loaded successfully; a transient
            // TTL-refresh failure must not turn a valid session into a 5xx.
            warn!(error = %err, session_id, "failed to refresh MCP session valkey ttl");
        }
        Ok(Some(state))
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        let key = mcp_session_cache_key(session_id);
        let payload =
            serde_json::to_string(state).map_err(|err| -> SessionStoreError { Box::new(err) })?;
        let mut manager = self.manager.clone();
        if let Err(err) = manager
            .set_ex::<_, _, ()>(key, payload, self.ttl_seconds)
            .await
        {
            warn!(error = %err, session_id, "failed to store MCP session in valkey");
            return Err(redis_session_error("store", err));
        }
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        let key = mcp_session_cache_key(session_id);
        let mut manager = self.manager.clone();
        if let Err(err) = manager.del::<_, usize>(key).await {
            warn!(error = %err, session_id, "failed to delete MCP session from valkey");
            return Err(redis_session_error("delete", err));
        }
        Ok(())
    }
}

fn redis_session_error(
    operation: &'static str,
    err: impl std::error::Error + Send + Sync + 'static,
) -> SessionStoreError {
    // SessionStoreError is boxed; a concrete error type keeps the failure
    // distinguishable from a plain session miss so callers can return 5xx
    // instead of `session_not_found`.
    Box::new(RedisSessionError {
        operation,
        source: Box::new(err),
    })
}

#[derive(Debug)]
struct RedisSessionError {
    operation: &'static str,
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl std::fmt::Display for RedisSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "valkey MCP session {} failed: {}",
            self.operation, self.source
        )
    }
}

impl std::error::Error for RedisSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

fn mcp_session_cache_key(session_id: &str) -> String {
    format!("{MCP_SESSION_VALKEY_KEY_PREFIX}{session_id}")
}
