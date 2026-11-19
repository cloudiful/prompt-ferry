use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::RwLock;

use crate::{
    relay_secrets::RelaySecretManager,
    standalone_config::{StandaloneConfig, StandaloneConfigStore},
    worker_usage::{StandaloneUsageSummary, UsageLog},
};

use super::StandaloneUsageBuffer;

#[derive(Clone)]
pub(crate) struct StandaloneRuntimeState {
    pub(super) store: Arc<StandaloneConfigStore>,
    manager: RelaySecretManager,
    snapshot: Arc<RwLock<StandaloneConfig>>,
    redaction_enabled: Arc<AtomicBool>,
    recent_usage: Arc<StandaloneUsageBuffer>,
    mcp_runtime: Option<crate::mcp::McpRuntimeState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientKeyIdentity {
    pub(crate) user_id: i64,
    pub(crate) label: String,
}

impl StandaloneRuntimeState {
    pub(crate) fn mcp_runtime(&self) -> Option<crate::mcp::McpRuntimeState> {
        self.mcp_runtime.clone()
    }

    /// Expose the underlying SQLite pool so the runtime's request-lease
    /// guard can spawn its heartbeat task without owning the full
    /// configuration snapshot. Cloning the pool handle is cheap and
    /// keeps `StandaloneRuntimeState` decoupled from lease internals.
    pub(crate) fn store_pool(&self) -> sqlx::SqlitePool {
        self.store.pool().clone()
    }

    /// Issue #599 R2c: config-repository handle for runtime paths that must
    /// read decrypted endpoint credentials (the ChatGPT OAuth token) while the
    /// data plane runs without an admin state of its own. Construction is
    /// cheap (Arc store + key material) and configuration writes keep going
    /// through the store's typed methods.
    pub(crate) fn config_repository(&self) -> crate::db::ConfigRepository {
        crate::db::ConfigRepository::sqlite(self.store.clone(), self.manager.clone())
    }

    pub(crate) async fn snapshot(&self) -> StandaloneConfig {
        self.snapshot.read().await.clone()
    }

    pub(crate) fn redaction_enabled(&self) -> bool {
        self.redaction_enabled.load(Ordering::SeqCst)
    }

    pub(crate) fn record_usage(&self, log: UsageLog) -> StandaloneUsageSummary {
        self.recent_usage.record(log)
    }

    pub(crate) async fn persist_usage(&self, summary: &StandaloneUsageSummary) {
        self.recent_usage.persist(summary).await;
    }

    pub(crate) async fn client_key_identity(&self, key_hash: &str) -> Option<ClientKeyIdentity> {
        let snapshot = self.snapshot.read().await;
        snapshot
            .client_keys
            .iter()
            .filter(|key| key.enabled)
            .find(|key| crate::keys::hash_client_key(&key.secret) == key_hash)
            .map(|key| ClientKeyIdentity {
                user_id: key.user_id,
                label: key.label.clone(),
            })
    }
}
