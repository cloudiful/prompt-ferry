use std::{sync::Arc, time::Duration};

use anyhow::Result;
use redis::{AsyncCommands, aio::ConnectionManager};
use rmcp::transport::streamable_http_server::session::{
    SessionState, SessionStore, SessionStoreError,
};
use tracing::warn;

use crate::{config::WorkerConfig, db::SharedValkeyConfig};

pub const MCP_SESSION_VALKEY_KEY_PREFIX: &str = "pfy:mcp-session:";

/// Bounded wait for the initial Valkey handshake. Without this wrapper
/// `get_connection_manager` follows the OS TCP timeout (~60-120s) and
/// the worker startup hangs even though the rest of the admin state is
/// already ready; on timeout we fall back to the process-local rmcp
/// session store so the first request can still proceed.
const VALKEY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The TTL of a session stored in the shared Valkey.
///
/// The shared TTL is the coordination contract between the workers: a session
/// must expire everywhere at the same time, so the local session TTL does not
/// apply to the shared backend.
pub(super) fn valkey_session_ttl_seconds(shared: &SharedValkeyConfig) -> u64 {
    shared.ttl_seconds().max(1)
}

/// The TTL of a session stored in the standalone SQLite coordinator.
pub(super) fn sqlite_session_ttl_seconds(config: &WorkerConfig) -> u64 {
    config.session_ttl_seconds.max(1)
}

#[derive(Clone)]
pub struct McpSessionStore {
    backend: McpSessionBackend,
    ttl_seconds: u64,
}

#[derive(Clone)]
enum McpSessionBackend {
    Valkey(ConnectionManager),
    Sqlite(crate::standalone_config::StandaloneCoordinatorStore),
}

impl McpSessionStore {
    pub async fn from_config(
        config: &WorkerConfig,
        shared: &SharedValkeyConfig,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        Self::from_config_with_sqlite(config, shared, None).await
    }

    pub async fn from_config_with_sqlite(
        config: &WorkerConfig,
        shared: &SharedValkeyConfig,
        sqlite_pool: Option<sqlx::SqlitePool>,
    ) -> Result<Option<Arc<dyn SessionStore>>> {
        let url = shared.url_or_empty().trim();
        if url.is_empty() {
            if let Some(pool) = sqlite_pool {
                warn!(
                    backend = "sqlite",
                    scope = "single-host",
                    network_filesystem_safe = false,
                    "using WAL-backed SQLite coordinator for MCP sessions"
                );
                return Ok(Some(Arc::new(Self {
                    backend: McpSessionBackend::Sqlite(
                        crate::standalone_config::StandaloneCoordinatorStore::new(pool),
                    ),
                    ttl_seconds: sqlite_session_ttl_seconds(config),
                })));
            }
            warn!(
                backend = "memory",
                scope = "single-process",
                capability = "mcp_session_cache",
                "using rmcp process-local MCP sessions; session loss on restart is accepted"
            );
            return Ok(None);
        }
        let client = redis::Client::open(url).map_err(|_| {
            crate::db::shared_valkey_failure(crate::db::SharedValkeyFailure::ClientOpen, url)
        })?;
        let manager =
            match tokio::time::timeout(VALKEY_CONNECT_TIMEOUT, client.get_connection_manager())
                .await
            {
                Ok(Ok(manager)) => manager,
                Ok(Err(_)) => {
                    // A configured shared Valkey is authoritative: a consumer that
                    // cannot reach it must fail instead of silently degrading to
                    // process-local sessions.
                    return Err(crate::db::shared_valkey_failure(
                        crate::db::SharedValkeyFailure::Connect,
                        url,
                    ));
                }
                Err(_) => {
                    return Err(crate::db::shared_valkey_failure(
                        crate::db::SharedValkeyFailure::Timeout,
                        url,
                    ));
                }
            };
        Ok(Some(Arc::new(Self {
            backend: McpSessionBackend::Valkey(manager),
            // The shared TTL is the coordination contract between the workers:
            // a session must expire everywhere at the same time.
            ttl_seconds: valkey_session_ttl_seconds(shared),
        })))
    }
}

#[async_trait::async_trait]
impl SessionStore for McpSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        let key = mcp_session_cache_key(session_id);
        let value = match &self.backend {
            McpSessionBackend::Valkey(manager) => {
                let mut manager = manager.clone();
                match manager.get(&key).await {
                    Ok(value) => {
                        if let Err(err) = manager
                            .expire::<_, bool>(
                                &key,
                                i64::try_from(self.ttl_seconds).unwrap_or(i64::MAX),
                            )
                            .await
                        {
                            warn!(error = %err, session_id, "failed to refresh MCP session valkey ttl");
                        }
                        value
                    }
                    Err(err) => {
                        warn!(error = %err, session_id, "failed to load MCP session from valkey");
                        return Err(redis_session_error("load", err));
                    }
                }
            }
            McpSessionBackend::Sqlite(store) => store
                .get("mcp-session", &key)
                .await
                .map_err(|err| sqlite_session_error("load", err))?,
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
        if let McpSessionBackend::Sqlite(store) = &self.backend {
            store
                .put("mcp-session", &key, &value, self.ttl_seconds)
                .await
                .map_err(|err| sqlite_session_error("refresh", err))?;
        }
        Ok(Some(state))
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        let payload =
            serde_json::to_string(state).map_err(|err| -> SessionStoreError { Box::new(err) })?;
        let key = mcp_session_cache_key(session_id);
        match &self.backend {
            McpSessionBackend::Valkey(manager) => {
                let mut manager = manager.clone();
                if let Err(err) = manager
                    .set_ex::<_, _, ()>(key, payload, self.ttl_seconds)
                    .await
                {
                    warn!(error = %err, session_id, "failed to store MCP session in valkey");
                    return Err(redis_session_error("store", err));
                }
            }
            McpSessionBackend::Sqlite(store) => store
                .put("mcp-session", &key, &payload, self.ttl_seconds)
                .await
                .map_err(|err| sqlite_session_error("store", err))?,
        }
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        let key = mcp_session_cache_key(session_id);
        match &self.backend {
            McpSessionBackend::Valkey(manager) => {
                let mut manager = manager.clone();
                if let Err(err) = manager.del::<_, usize>(key).await {
                    warn!(error = %err, session_id, "failed to delete MCP session from valkey");
                    return Err(redis_session_error("delete", err));
                }
            }
            McpSessionBackend::Sqlite(store) => store
                .delete("mcp-session", &key)
                .await
                .map(|_| ())
                .map_err(|err| sqlite_session_error("delete", err))?,
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

fn sqlite_session_error(operation: &'static str, err: anyhow::Error) -> SessionStoreError {
    Box::new(SessionBackendError {
        operation,
        message: err.to_string(),
    })
}

#[derive(Debug)]
struct SessionBackendError {
    operation: &'static str,
    message: String,
}

impl std::fmt::Display for SessionBackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "SQLite MCP session {} failed: {}",
            self.operation, self.message
        )
    }
}

impl std::error::Error for SessionBackendError {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_valkey_session_uses_the_shared_ttl_instead_of_the_local_one() {
        let config = WorkerConfig {
            session_ttl_seconds: 42,
            ..WorkerConfig::default()
        };
        let shared = SharedValkeyConfig::resolved(None, 900, Some(3));
        assert_eq!(valkey_session_ttl_seconds(&shared), 900);
        assert_eq!(sqlite_session_ttl_seconds(&config), 42);
    }
}
