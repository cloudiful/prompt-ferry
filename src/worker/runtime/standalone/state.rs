use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

use anyhow::Context;
use tokio::sync::{Mutex, RwLock, mpsc};
use uuid::Uuid;

use crate::{
    bridge::protocol::BridgeMessage,
    db::ManagedRelayRuntimeStatus,
    relay_secrets::RelaySecretManager,
    standalone_config::{StandaloneConfig, StandaloneConfigStore},
    worker_usage::{StandaloneUsageSummary, UsageLog},
};

use super::{DEFAULT_USAGE_CAPACITY, StandaloneUsageBuffer};

#[derive(Clone)]
pub(crate) struct StandaloneRuntimeState {
    pub(super) store: Arc<StandaloneConfigStore>,
    manager: RelaySecretManager,
    snapshot: Arc<RwLock<StandaloneConfig>>,
    pub(super) relay_senders:
        Arc<Mutex<std::collections::HashMap<String, mpsc::UnboundedSender<BridgeMessage>>>>,
    pub(super) relay_statuses:
        Arc<RwLock<std::collections::HashMap<Uuid, ManagedRelayRuntimeStatus>>>,
    pub(super) snapshot_version: Arc<AtomicI64>,
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
    pub(crate) async fn reload_snapshot(&self) -> anyhow::Result<bool> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .context("failed to reload standalone configuration")?;
        let changed = {
            let mut current = self.snapshot.write().await;
            if *current == snapshot {
                false
            } else {
                *current = snapshot.clone();
                true
            }
        };
        if changed {
            apply_redaction_settings(&snapshot, &self.redaction_enabled);
        }
        Ok(changed)
    }

    pub(crate) fn with_mcp_runtime(mut self, mcp_runtime: crate::mcp::McpRuntimeState) -> Self {
        self.mcp_runtime = Some(mcp_runtime);
        self
    }

    pub(crate) fn mcp_runtime(&self) -> Option<crate::mcp::McpRuntimeState> {
        self.mcp_runtime.clone()
    }

    pub(crate) fn new(
        store: Arc<StandaloneConfigStore>,
        manager: RelaySecretManager,
        snapshot: StandaloneConfig,
    ) -> Self {
        let redaction_enabled = Arc::new(AtomicBool::new(false));
        apply_redaction_settings(&snapshot, &redaction_enabled);
        Self {
            store: store.clone(),
            manager,
            snapshot: Arc::new(RwLock::new(snapshot)),
            relay_senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            relay_statuses: Arc::new(RwLock::new(std::collections::HashMap::new())),
            snapshot_version: Arc::new(AtomicI64::new(0)),
            redaction_enabled,
            recent_usage: Arc::new(StandaloneUsageBuffer::with_persistence(
                DEFAULT_USAGE_CAPACITY,
                store,
            )),
            mcp_runtime: None,
        }
    }

    /// Restore the in-memory usage cache from the durable SQLite ledger so
    /// reopening the same database returns the recent bounded summaries.
    /// Persistence errors degrade gracefully to an empty cache.
    pub(crate) async fn hydrate_usage(&self) {
        self.recent_usage.hydrate_from_store().await;
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

    pub(crate) async fn set_bridge_sender(
        &self,
        relay_key: &str,
        sender: Option<mpsc::UnboundedSender<BridgeMessage>>,
    ) {
        let mut senders = self.relay_senders.lock().await;
        match sender {
            Some(sender) => {
                senders.insert(relay_key.to_string(), sender);
            }
            None => {
                senders.remove(relay_key);
            }
        }
    }

    pub(crate) async fn mark_connected(&self, relay_id: Uuid) {
        let mut statuses = self.relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = true;
        status.last_error = None;
        status.last_connected_at = Some(chrono::Utc::now());
    }

    pub(crate) async fn mark_disconnected(&self, relay_id: Uuid, error: Option<String>) {
        let mut statuses = self.relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = false;
        status.last_disconnected_at = Some(chrono::Utc::now());
        if let Some(error) = error {
            status.last_error = Some(error);
        }
    }
}

pub(crate) fn apply_redaction_settings(snapshot: &StandaloneConfig, enabled: &AtomicBool) {
    let config = snapshot
        .settings
        .iter()
        .find(|setting| setting.key == "redaction_config")
        .and_then(|setting| {
            serde_json::from_value::<crate::redact::RedactionConfig>(setting.value.clone()).ok()
        });
    let applied = config
        .as_ref()
        .filter(|config| config.validate().is_ok())
        .and_then(|config| crate::redact::apply_config(config).ok())
        .is_some();
    if !applied {
        let _ = crate::redact::apply_config(&crate::redact::RedactionConfig::default());
    }
    enabled.store(
        applied && config.is_some_and(|config| config.enabled),
        Ordering::SeqCst,
    );
}
