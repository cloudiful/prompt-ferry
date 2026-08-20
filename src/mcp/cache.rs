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
        let url = config.valkey_url.trim();
        if url.is_empty() {
            return Self::new();
        }

        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to open valkey client for MCP catalog; falling back to local cache");
                return Self::new();
            }
        };
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to connect valkey for MCP catalog; falling back to local cache");
                return Self::new();
            }
        };

        Self {
            backend: CacheBackend::Valkey(manager),
            local: Arc::new(RwLock::new(HashMap::new())),
            refreshing: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub async fn get(&self, server: &McpServer) -> Option<ServerCatalogSnapshot> {
        let CacheBackend::Valkey(manager) = &self.backend else {
            return self.local_snapshot(server).await;
        };
        let key = catalog_key(server.server_id);
        let mut manager = manager.clone();
        let value: Option<String> = match manager.get(&key).await {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, server_id = %server.server_id, "failed to read MCP catalog from valkey");
                return self.local_snapshot(server).await;
            }
        };
        let value = value?;
        let stored: StoredCatalog = match serde_json::from_str(&value) {
            Ok(stored) => stored,
            Err(err) => {
                warn!(error = %err, server_id = %server.server_id, "invalid MCP catalog valkey payload");
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
}
