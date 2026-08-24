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

use super::{
    DEFAULT_USAGE_CAPACITY, StandaloneFeature, StandaloneFeatureDiagnostic, StandaloneUsageBuffer,
    standalone_feature_diagnostic, standalone_feature_diagnostics,
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
    recent_usage: Arc<StandaloneUsageBuffer>,
    mcp_runtime: Option<crate::mcp::McpRuntimeState>,
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

    /// Load the latest durable replay snapshot for a conversation, if any
    /// has been persisted. Used by runtime replay consumers (for example,
    /// prompt reconstruction) to restore the most recent prompt-ref
    /// checkpoint after a process restart. Errors are reported via the
    /// shared `StandaloneConfigError` taxonomy so callers can decide
    /// whether to skip or fail their load path.
    pub(crate) async fn latest_replay_snapshot(
        &self,
        conversation_id: Uuid,
    ) -> anyhow::Result<Option<crate::standalone_config::StandaloneReplaySnapshotRecord>> {
        Ok(self.store.get_replay_snapshot(conversation_id).await?)
    }

    pub(crate) fn with_mcp_runtime(mut self, mcp_runtime: crate::mcp::McpRuntimeState) -> Self {
        self.mcp_runtime = Some(mcp_runtime);
        self
    }

    pub(crate) fn mcp_runtime(&self) -> Option<crate::mcp::McpRuntimeState> {
        self.mcp_runtime.clone()
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

    pub(crate) fn record_usage(&self, log: UsageLog) -> StandaloneUsageSummary {
        self.recent_usage.record(log)
    }

    pub(crate) async fn persist_usage(&self, summary: &StandaloneUsageSummary) {
        self.recent_usage.persist(summary).await;
    }

    #[allow(dead_code)]
    pub(crate) fn recent_usage(&self) -> Vec<StandaloneUsageSummary> {
        self.recent_usage.snapshot()
    }

    #[allow(dead_code)]
    pub(crate) fn feature_diagnostics(&self) -> [StandaloneFeatureDiagnostic; 7] {
        standalone_feature_diagnostics()
    }

    #[allow(dead_code)]
    pub(crate) fn feature_diagnostic(
        &self,
        feature: StandaloneFeature,
    ) -> StandaloneFeatureDiagnostic {
        standalone_feature_diagnostic(feature)
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
        standalone_config::{
            ClientKeyConfig, SettingConfig, StandaloneConfigStore, StandaloneUsageSummaryRecord,
        },
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
    async fn new_state_starts_with_empty_disposable_usage() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let state = StandaloneRuntimeState::new(
            store,
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager"),
            StandaloneConfig::default(),
        );

        assert!(state.recent_usage().is_empty());
        assert_eq!(state.feature_diagnostics().len(), 7);
        assert_eq!(
            state.feature_diagnostic(StandaloneFeature::Mcp).code,
            "sqlite_mcp_quota_unavailable"
        );
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

    #[tokio::test]
    async fn standalone_runtime_persists_usage_and_hydrates_on_reopen() {
        use crate::worker_usage::{UsageLog, UsageRequestMetadata};

        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");

        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let first_state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        first_state.hydrate_usage().await;
        assert!(first_state.recent_usage().is_empty());

        let first_request = uuid::Uuid::new_v4();
        let second_request = uuid::Uuid::new_v4();
        for (request_id, path_label, model) in [
            (first_request, "/v1/responses", "gpt-5"),
            (second_request, "/v1/chat", "claude"),
        ] {
            let summary = first_state.record_usage(
                UsageLog::ai_request(
                    request_id,
                    UsageRequestMetadata {
                        path: path_label.to_string(),
                        ..UsageRequestMetadata::default()
                    },
                    Some(model.to_string()),
                )
                .with_status(Some(200), Some(true), Some(150), Some(20)),
            );
            // Persistence is awaited inline so the SQLite write is durable
            // before this test continues; no fire-and-forget is left to be
            // lost by the runtime's drain on shutdown.
            first_state.persist_usage(&summary).await;
        }

        let persisted = store
            .list_usage_summaries(64)
            .await
            .expect("persisted list");
        let persisted_ids = persisted
            .iter()
            .map(|record| record.request_id)
            .collect::<Vec<_>>();
        assert!(persisted_ids.contains(&first_request));
        assert!(persisted_ids.contains(&second_request));

        drop(first_state);
        drop(store);

        let reopened_store = Arc::new(StandaloneConfigStore::open(&path).await.expect("reopen"));
        let reopened_state = StandaloneRuntimeState::new(
            reopened_store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        reopened_state.hydrate_usage().await;
        let hydrated = reopened_state.recent_usage();
        let hydrated_ids = hydrated
            .iter()
            .map(|summary| summary.request_id)
            .collect::<Vec<_>>();
        assert!(
            hydrated_ids.contains(&first_request),
            "first persisted request must hydrate after reopen"
        );
        assert!(
            hydrated_ids.contains(&second_request),
            "second persisted request must hydrate after reopen"
        );

        drop(reopened_state);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn standalone_runtime_persists_repeated_lifecycle_events_for_one_request() {
        use crate::db::{RequestRecordState, UsageEventKind};
        use crate::worker_usage::{UsageLog, UsageRequestMetadata};

        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));

        let request_id = uuid::Uuid::new_v4();
        let initial = UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        );
        let terminal = UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        )
        .with_state(UsageEventKind::Request, RequestRecordState::Failed)
        .with_status(Some(502), Some(false), Some(40), None)
        .with_error(Some("upstream_error".to_string()), None, None);

        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let summary_initial = state.record_usage(initial);
        state.persist_usage(&summary_initial).await;
        let summary_terminal = state.record_usage(terminal);
        state.persist_usage(&summary_terminal).await;

        let stored = store
            .list_usage_summaries(64)
            .await
            .expect("persisted list");
        assert_eq!(
            stored.len(),
            2,
            "both lifecycle events for the same request must be retained"
        );
        let states = stored
            .iter()
            .map(|record| record.state.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec!["received", "failed"],
            "ledger must keep the original Received event and the terminal Failed event"
        );

        drop(state);
        drop(store);

        let reopened = Arc::new(StandaloneConfigStore::open(&path).await.expect("reopen"));
        reopened_state_assert_terminal_state(reopened, request_id).await;

        let _ = std::fs::remove_file(path);
    }

    async fn reopened_state_assert_terminal_state(
        store: Arc<StandaloneConfigStore>,
        request_id: uuid::Uuid,
    ) {
        let records = store.list_usage_summaries(64).await.expect("reopen list");
        let for_request: Vec<&StandaloneUsageSummaryRecord> = records
            .iter()
            .filter(|record| record.request_id == request_id)
            .collect();
        assert_eq!(for_request.len(), 2);
        let states: Vec<&str> = for_request
            .iter()
            .map(|record| record.state.as_str())
            .collect();
        assert_eq!(states, vec!["received", "failed"]);
    }

    #[tokio::test]
    async fn standalone_runtime_persists_phase_1c_a_request_metadata_and_hydrates_it() {
        use crate::db::{RequestRecordState, UsageEventKind};
        use crate::worker_usage::{UsageLog, UsageRequestMetadata};

        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));

        let request_id = uuid::Uuid::new_v4();
        let initial = UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                client_key_id: Some(7),
                client_key_label: Some("primary".to_string()),
                request_user_agent: Some("prompt-ferry-cli/0.4".to_string()),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        )
        .with_endpoint_key(Some(uuid::Uuid::new_v4()), Some("primary".to_string()))
        .with_response(
            Some("resp_001".to_string()),
            Some("conv-001".to_string()),
            None,
            None,
        )
        .with_error(None, Some("upstream returned HTTP 502".to_string()), None);

        let terminal = UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                client_key_id: Some(7),
                client_key_label: Some("primary".to_string()),
                request_user_agent: Some("prompt-ferry-cli/0.4".to_string()),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        )
        .with_endpoint_key(Some(uuid::Uuid::new_v4()), Some("primary".to_string()))
        .with_response(
            Some("resp_001".to_string()),
            Some("conv-001".to_string()),
            None,
            None,
        )
        .with_state(UsageEventKind::Request, RequestRecordState::Failed)
        .with_status(Some(502), Some(false), Some(40), None)
        .with_error(Some("upstream_error".to_string()), None, None);

        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let summary_initial = state.record_usage(initial);
        state.persist_usage(&summary_initial).await;
        let summary_terminal = state.record_usage(terminal);
        state.persist_usage(&summary_terminal).await;

        // Hydrate from the store while the runtime is still open so we can
        // verify the in-memory cache round-trips the new metadata fields
        // before restarting the database.
        state.hydrate_usage().await;
        let hydrated = state.recent_usage();
        assert_eq!(hydrated.len(), 2);
        for summary in &hydrated {
            assert_eq!(summary.client_key_id, Some(7));
            assert_eq!(summary.client_key_label.as_deref(), Some("primary"));
            assert_eq!(
                summary.request_user_agent.as_deref(),
                Some("prompt-ferry-cli/0.4")
            );
            assert_eq!(summary.endpoint_key_label.as_deref(), Some("primary"));
            assert_eq!(summary.provider_response_id.as_deref(), Some("resp_001"));
            assert_eq!(
                summary.provider_conversation_key.as_deref(),
                Some("conv-001")
            );
        }
        // The terminal event overwrote the initial error_message; confirm the
        // distinct value survived the lifecycle.
        assert_eq!(
            hydrated[0].error_message.as_deref(),
            Some("upstream returned HTTP 502")
        );
        assert_eq!(hydrated[1].error_message.as_deref(), None);

        drop(state);
        drop(store);

        let reopened_store = Arc::new(StandaloneConfigStore::open(&path).await.expect("reopen"));
        let reopened_state = StandaloneRuntimeState::new(
            reopened_store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        reopened_state.hydrate_usage().await;
        let after_reopen = reopened_state.recent_usage();
        assert_eq!(after_reopen.len(), 2);
        for summary in &after_reopen {
            assert_eq!(summary.client_key_id, Some(7));
            assert_eq!(summary.client_key_label.as_deref(), Some("primary"));
            assert_eq!(
                summary.request_user_agent.as_deref(),
                Some("prompt-ferry-cli/0.4")
            );
            assert_eq!(summary.provider_response_id.as_deref(), Some("resp_001"));
            assert_eq!(
                summary.provider_conversation_key.as_deref(),
                Some("conv-001")
            );
        }

        drop(reopened_state);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn standalone_runtime_persists_phase_1c_b_replay_snapshots() {
        use crate::worker_usage::{UsageLog, UsageRequestMetadata};

        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));

        let conversation_id = Uuid::new_v4();
        let initial = UsageLog::ai_request(
            Uuid::new_v4(),
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                conversation_id: Some(conversation_id),
                conversation_seq: Some(1),
                snapshot_prompt_refs_json: Some(serde_json::json!([
                    {"role": "user", "block_hash": "hash-1"}
                ])),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        );
        let terminal = UsageLog::ai_request(
            Uuid::new_v4(),
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                conversation_id: Some(conversation_id),
                conversation_seq: Some(2),
                snapshot_prompt_refs_json: Some(serde_json::json!([
                    {"role": "user", "block_hash": "hash-1"},
                    {"role": "assistant", "block_hash": "hash-2"}
                ])),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        )
        .with_state(
            crate::db::UsageEventKind::Request,
            crate::db::RequestRecordState::Completed,
        );

        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let initial_summary = state.record_usage(initial);
        state.persist_usage(&initial_summary).await;
        let terminal_summary = state.record_usage(terminal);
        state.persist_usage(&terminal_summary).await;

        // Reject older sequence: a synthetic snapshot with the same
        // conversation id at a lower conversation_seq must not regress
        // the stored row even though `persist_usage` runs again with
        // stale in-memory data.
        let stale = UsageLog::ai_request(
            Uuid::new_v4(),
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                conversation_id: Some(conversation_id),
                conversation_seq: Some(1),
                snapshot_prompt_refs_json: Some(serde_json::json!([
                    {"role": "user", "block_hash": "stale"}
                ])),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        );
        let stale_summary = state.record_usage(stale);
        state.persist_usage(&stale_summary).await;

        let loaded = store
            .get_replay_snapshot(conversation_id)
            .await
            .expect("get snapshot")
            .expect("stored snapshot");
        assert_eq!(loaded.conversation_seq, 2);
        assert_eq!(loaded.ref_count, 2);
        let hydrated = state
            .latest_replay_snapshot(conversation_id)
            .await
            .expect("runtime hydrate")
            .expect("runtime snapshot");
        assert_eq!(hydrated.conversation_seq, 2);
        assert_eq!(hydrated.ref_count, 2);

        // Restart: reopen the same SQLite file and confirm the
        // snapshot hydrates without losing the higher sequence.
        drop(state);
        drop(store);

        let reopened_store = Arc::new(
            StandaloneConfigStore::open(&path)
                .await
                .expect("reopen store"),
        );
        let reopened_state = StandaloneRuntimeState::new(
            reopened_store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let reopened_snapshot = reopened_state
            .latest_replay_snapshot(conversation_id)
            .await
            .expect("reopen hydrate")
            .expect("reopen snapshot");
        assert_eq!(reopened_snapshot.conversation_seq, 2);
        assert_eq!(reopened_snapshot.ref_count, 2);

        drop(reopened_state);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn standalone_runtime_skips_replay_snapshot_for_malformed_prompt_refs() {
        use crate::worker_usage::{UsageLog, UsageRequestMetadata};

        let path = database_path();
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));

        let conversation_id = Uuid::new_v4();
        // Malformed JSON for `snapshot_prompt_refs_json` must warn and
        // leave the request path usable; the usage summary still lands
        // and no replay row is written.
        let malformed = UsageLog::ai_request(
            Uuid::new_v4(),
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                conversation_id: Some(conversation_id),
                conversation_seq: Some(3),
                snapshot_prompt_refs_json: Some(serde_json::json!("not-an-array")),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        );

        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let summary = state.record_usage(malformed);
        state.persist_usage(&summary).await;

        let stored = store
            .list_usage_summaries(64)
            .await
            .expect("usage summary persisted despite malformed refs");
        assert!(
            stored
                .iter()
                .any(|record| record.request_id == summary.request_id),
            "usage summary row must still be persisted when the replay refs are malformed"
        );
        let snapshot = store
            .get_replay_snapshot(conversation_id)
            .await
            .expect("get snapshot");
        assert!(
            snapshot.is_none(),
            "malformed prompt refs must not write a replay snapshot row"
        );

        drop(state);
        let _ = std::fs::remove_file(path);
    }
}
