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
};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientKeyIdentity {
    pub(crate) user_id: i64,
    pub(crate) label: String,
}

impl StandaloneRuntimeState {
    pub(crate) fn new(
        store: Arc<StandaloneConfigStore>,
        manager: RelaySecretManager,
        snapshot: StandaloneConfig,
    ) -> Self {
        let redaction_enabled = Arc::new(AtomicBool::new(false));
        apply_redaction_settings(&snapshot, &redaction_enabled);
        Self {
            store,
            manager,
            snapshot: Arc::new(RwLock::new(snapshot)),
            relay_senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            relay_statuses: Arc::new(RwLock::new(std::collections::HashMap::new())),
            snapshot_version: Arc::new(AtomicI64::new(0)),
            redaction_enabled,
        }
    }

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

    pub(crate) async fn snapshot(&self) -> StandaloneConfig {
        self.snapshot.read().await.clone()
    }

    pub(crate) fn redaction_enabled(&self) -> bool {
        self.redaction_enabled.load(Ordering::SeqCst)
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

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use uuid::Uuid;

    use super::*;
    use crate::{
        redact::RedactionConfig,
        relay_secrets::RelaySecretManager,
        standalone_config::{ClientKeyConfig, SettingConfig, StandaloneConfigStore},
    };

    fn database_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-runtime-state-{suffix}.sqlite"))
    }

    #[tokio::test]
    async fn resolves_client_key_identity_from_loaded_snapshot() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let state = StandaloneRuntimeState::new(
            store,
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager"),
            StandaloneConfig {
                client_keys: vec![ClientKeyConfig {
                    key_id: Uuid::new_v4(),
                    user_id: 42,
                    key_prefix: "pfy_test".to_string(),
                    label: "test key".to_string(),
                    secret: "client-secret".to_string(),
                    enabled: true,
                }],
                ..StandaloneConfig::default()
            },
        );

        let identity = state
            .client_key_identity(&crate::keys::hash_client_key("client-secret"))
            .await
            .expect("identity");
        assert_eq!(identity.user_id, 42);
        assert_eq!(identity.label, "test key");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn loads_persisted_redaction_setting_and_applies_redaction() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let config = RedactionConfig {
            enabled: true,
            custom_strings: vec![redactor::CustomStringRule {
                pattern: "standalone-secret".to_string(),
                match_type: redactor::CustomStringMatch::Exact,
                scope: redactor::CustomStringScope::Text,
            }],
            ..RedactionConfig::default()
        };
        let persisted = StandaloneConfig {
            settings: vec![SettingConfig {
                key: "redaction_config".to_string(),
                version: 1,
                value: serde_json::to_value(config).expect("redaction config JSON"),
            }],
            ..StandaloneConfig::default()
        };
        store
            .replace_snapshot(&manager, &persisted)
            .await
            .expect("persist redaction setting");
        let loaded = store.load_snapshot(&manager).await.expect("load snapshot");
        let state = StandaloneRuntimeState::new(store, manager, loaded);

        assert!(state.redaction_enabled());
        let redacted = crate::redact::redact_text("standalone-secret");
        assert!(!redacted.contains("standalone-secret"));
        drop(state);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn invalid_or_missing_redaction_setting_uses_disabled_default() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));

        for settings in [
            vec![SettingConfig {
                key: "redaction_config".to_string(),
                version: 1,
                value: serde_json::json!({"enabled": "invalid"}),
            }],
            Vec::new(),
        ] {
            let state = StandaloneRuntimeState::new(
                store.clone(),
                manager.clone(),
                StandaloneConfig {
                    settings,
                    ..StandaloneConfig::default()
                },
            );
            assert!(!state.redaction_enabled());
            assert_eq!(
                crate::redact::redact_text("standalone-secret"),
                "standalone-secret"
            );
        }

        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
