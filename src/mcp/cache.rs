use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use crate::{config::WorkerConfig, db::McpServer};

pub const MCP_CATALOG_VALKEY_KEY_PREFIX: &str = "pfy:mcp-catalog:";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerCatalogSnapshot {
    pub tools: Vec<Value>,
    pub resources: Vec<Value>,
    #[serde(default)]
    pub resource_templates: Vec<Value>,
    pub prompts: Vec<Value>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    updated_at: DateTime<Utc>,
    snapshot: ServerCatalogSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredCatalog {
    updated_at: DateTime<Utc>,
    snapshot: ServerCatalogSnapshot,
}

#[derive(Clone)]
enum CacheBackend {
    Local,
    Valkey(ConnectionManager),
    Sqlite(SqliteCatalogBackend),
}

#[derive(Clone)]
struct SqliteCatalogBackend {
    coordinator: crate::standalone_config::StandaloneCoordinatorStore,
    ttl_seconds: u64,
}

#[derive(Clone)]
pub struct McpCatalogCache {
    backend: CacheBackend,
    local: Arc<RwLock<HashMap<Uuid, CacheEntry>>>,
    refreshing: Arc<RwLock<HashSet<Uuid>>>,
}

impl McpCatalogCache {
    pub fn new() -> Self {
        Self {
            backend: CacheBackend::Local,
            local: Arc::new(RwLock::new(HashMap::new())),
            refreshing: Arc::new(RwLock::new(HashSet::new())),
        }
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
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
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
        };

        Self {
            backend: CacheBackend::Valkey(manager),
            local: Arc::new(RwLock::new(HashMap::new())),
            refreshing: Arc::new(RwLock::new(HashSet::new())),
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
            refreshing: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub async fn get(&self, server: &McpServer) -> Option<ServerCatalogSnapshot> {
        let value = match &self.backend {
            CacheBackend::Local => return self.local_snapshot(server).await,
            CacheBackend::Valkey(manager) => {
                let key = catalog_key(server.server_id);
                let mut manager = manager.clone();
                match manager.get(&key).await {
                    Ok(value) => value,
                    Err(err) => {
                        warn!(error = %err, server_id = %server.server_id, "failed to read MCP catalog from valkey");
                        return self.local_snapshot(server).await;
                    }
                }
            }
            CacheBackend::Sqlite(store) => match store
                .coordinator
                .get("mcp-catalog", &server.server_id.to_string())
                .await
            {
                Ok(value) => value,
                Err(err) => {
                    warn!(error = %err, server_id = %server.server_id, "failed to read MCP catalog from SQLite");
                    return self.local_snapshot(server).await;
                }
            },
        }?;
        let stored: StoredCatalog = match serde_json::from_str(&value) {
            Ok(stored) => stored,
            Err(err) => {
                warn!(error = %err, server_id = %server.server_id, "invalid MCP catalog cache payload");
                return None;
            }
        };
        if stored.updated_at != server.updated_at {
            return None;
        }

        let snapshot = stored.snapshot;
        self.local.write().await.insert(
            server.server_id,
            CacheEntry {
                updated_at: stored.updated_at,
                snapshot: snapshot.clone(),
            },
        );
        Some(snapshot)
    }

    async fn local_snapshot(&self, server: &McpServer) -> Option<ServerCatalogSnapshot> {
        self.local
            .read()
            .await
            .get(&server.server_id)
            .filter(|entry| entry.updated_at == server.updated_at)
            .map(|entry| entry.snapshot.clone())
    }

    pub async fn put(&self, server: &McpServer, snapshot: ServerCatalogSnapshot) {
        self.local.write().await.insert(
            server.server_id,
            CacheEntry {
                updated_at: server.updated_at,
                snapshot: snapshot.clone(),
            },
        );

        let CacheBackend::Valkey(manager) = &self.backend else {
            if let CacheBackend::Sqlite(store) = &self.backend {
                if let Ok(payload) = serde_json::to_string(&StoredCatalog {
                    updated_at: server.updated_at,
                    snapshot,
                }) {
                    if let Err(err) = store
                        .coordinator
                        .put(
                            "mcp-catalog",
                            &server.server_id.to_string(),
                            &payload,
                            store.ttl_seconds,
                        )
                        .await
                    {
                        warn!(error = %err, server_id = %server.server_id, "failed to write MCP catalog to SQLite");
                    }
                }
            }
            return;
        };
        let payload = match serde_json::to_string(&StoredCatalog {
            updated_at: server.updated_at,
            snapshot,
        }) {
            Ok(payload) => payload,
            Err(err) => {
                warn!(error = %err, server_id = %server.server_id, "failed to serialize MCP catalog for valkey");
                return;
            }
        };
        let mut manager = manager.clone();
        if let Err(err) = manager
            .set::<_, _, ()>(catalog_key(server.server_id), payload)
            .await
        {
            warn!(error = %err, server_id = %server.server_id, "failed to write MCP catalog to valkey");
        }
    }

    pub async fn invalidate(&self, server_id: Uuid) {
        self.local.write().await.remove(&server_id);
        self.refreshing.write().await.remove(&server_id);

        let CacheBackend::Valkey(manager) = &self.backend else {
            if let CacheBackend::Sqlite(store) = &self.backend
                && let Err(err) = store
                    .coordinator
                    .delete("mcp-catalog", &server_id.to_string())
                    .await
            {
                warn!(error = %err, server_id = %server_id, "failed to delete MCP catalog from SQLite");
            }
            return;
        };
        let mut manager = manager.clone();
        if let Err(err) = manager.del::<_, usize>(catalog_key(server_id)).await {
            warn!(error = %err, server_id = %server_id, "failed to delete MCP catalog from valkey");
        }
    }

    pub async fn mark_refresh_inflight(&self, server_id: Uuid) -> bool {
        self.refreshing.write().await.insert(server_id)
    }

    pub async fn finish_refresh(&self, server_id: Uuid) {
        self.refreshing.write().await.remove(&server_id);
    }
}

impl Default for McpCatalogCache {
    fn default() -> Self {
        Self::new()
    }
}

fn catalog_key(server_id: Uuid) -> String {
    format!("{MCP_CATALOG_VALKEY_KEY_PREFIX}{server_id}")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn server() -> McpServer {
        McpServer {
            server_id: Uuid::new_v4(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "alpha".to_string(),
            aggregate_naming_mode: "qualified_only".to_string(),
            transport: "http".to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: json!([]),
            env_json: json!({}),
            bearer_tokens_json: json!([]),
            http_headers_json: json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: json!([]),
            disabled_tools: json!([]),
            disabled_resources: json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
            lifecycle_learned_mode: None,
            lifecycle_learned_protocol_version: None,
            lifecycle_learned_for_updated_at: None,
            lifecycle_learned_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn snapshot(name: &str) -> ServerCatalogSnapshot {
        ServerCatalogSnapshot {
            tools: vec![json!({ "name": name })],
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
        }
    }

    #[tokio::test]
    async fn local_cache_reuses_snapshot_without_expiry() {
        let cache = McpCatalogCache::new();
        let server = server();
        let expected = snapshot("cached");

        cache.put(&server, expected.clone()).await;

        assert_eq!(cache.get(&server).await, Some(expected));
    }

    #[tokio::test]
    async fn config_change_prevents_old_snapshot_reuse() {
        let cache = McpCatalogCache::new();
        let server = server();
        cache.put(&server, snapshot("cached")).await;

        let mut changed = server.clone();
        changed.updated_at += chrono::Duration::seconds(1);

        assert_eq!(cache.get(&changed).await, None);
    }

    #[tokio::test]
    async fn refresh_inflight_is_deduplicated() {
        let cache = McpCatalogCache::new();
        let server = server();

        assert!(cache.mark_refresh_inflight(server.server_id).await);
        assert!(!cache.mark_refresh_inflight(server.server_id).await);

        cache.finish_refresh(server.server_id).await;

        assert!(cache.mark_refresh_inflight(server.server_id).await);
    }

    #[tokio::test]
    async fn sqlite_catalog_cache_is_shared_between_cache_instances() {
        let path =
            std::env::temp_dir().join(format!("prompt-ferry-mcp-cache-{}.sqlite", Uuid::new_v4()));
        let pool = crate::db::connect_sqlite(&path).await.unwrap();
        crate::db::migrate_standalone(&pool).await.unwrap();
        let config = WorkerConfig::default();
        let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
        let second = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
        let server = server();
        let expected = snapshot("sqlite-cached");

        first.put(&server, expected.clone()).await;
        assert_eq!(second.get(&server).await, Some(expected));
        second.invalidate(server.server_id).await;
        assert_eq!(first.get(&server).await, None);
        pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
