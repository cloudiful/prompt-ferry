use std::sync::atomic::Ordering;

use crate::{
    bridge::protocol::{BridgeMessage, ClientRoute, ConfigSnapshot, RelayIpPolicy},
    ip_acl,
    keys::hash_client_key,
};

use super::state::StandaloneRuntimeState;

pub(crate) async fn publish_snapshot(state: &StandaloneRuntimeState) -> anyhow::Result<i64> {
    let snapshot = state.snapshot().await;
    let keys = snapshot
        .client_keys
        .iter()
        .filter(|key| key.enabled)
        .filter_map(|key| {
            let route_id = route_id_for_user(&snapshot, key.user_id)?;
            Some(ClientRoute {
                key_hash: hash_client_key(&key.secret),
                key_prefix: key.key_prefix.clone(),
                user_id: key.user_id,
                route_id: route_id.to_string(),
            })
        })
        .collect();
    let relay_ip_policy = relay_ip_policy(&snapshot);
    let version = state.snapshot_version.fetch_add(1, Ordering::SeqCst) + 1;
    let message = BridgeMessage::ConfigSnapshot(ConfigSnapshot {
        version,
        keys,
        relay_ip_policy,
    });
    let mut senders = state.relay_senders.lock().await;
    let relay_keys = senders.keys().cloned().collect::<Vec<_>>();
    let mut disconnected = Vec::new();
    for relay_key in relay_keys {
        let Some(sender) = senders.get(&relay_key) else {
            continue;
        };
        if sender.send(message.clone()).is_err() {
            disconnected.push(relay_key);
        } else if let Ok(relay_id) = relay_key.parse::<uuid::Uuid>() {
            let mut statuses = state.relay_statuses.write().await;
            if let Some(status) = statuses.get_mut(&relay_id) {
                status.last_snapshot_version = Some(version);
            }
        }
    }
    for relay_key in disconnected {
        senders.remove(&relay_key);
        if let Ok(relay_id) = relay_key.parse::<uuid::Uuid>() {
            state.mark_disconnected(relay_id, None).await;
        }
    }
    Ok(version)
}

fn route_id_for_user(
    snapshot: &crate::standalone_config::StandaloneConfig,
    user_id: i64,
) -> Option<uuid::Uuid> {
    [
        crate::standalone_config::RouteScope::User,
        crate::standalone_config::RouteScope::Admin,
    ]
    .into_iter()
    .find_map(|scope| {
        snapshot
            .routes
            .iter()
            .filter(|route| {
                route.enabled
                    && route.scope == scope
                    && (scope == crate::standalone_config::RouteScope::Admin
                        || route.owner_user_id == Some(user_id))
            })
            .flat_map(|route| route.targets.iter())
            .find_map(|target| {
                (target.enabled
                    && snapshot.endpoints.iter().any(|endpoint| {
                        endpoint.endpoint_id == target.endpoint_id && endpoint.enabled
                    }))
                .then_some(target.endpoint_id)
            })
    })
    .or_else(|| {
        snapshot
            .endpoints
            .iter()
            .find(|endpoint| endpoint.enabled)
            .map(|endpoint| endpoint.endpoint_id)
    })
}

fn relay_ip_policy(snapshot: &crate::standalone_config::StandaloneConfig) -> RelayIpPolicy {
    let Some(setting) = snapshot
        .settings
        .iter()
        .find(|setting| setting.key == "relay_ip_whitelist")
    else {
        return RelayIpPolicy::default();
    };
    let Ok(policy) = serde_json::from_value::<RelayIpPolicy>(setting.value.clone()) else {
        return RelayIpPolicy::default();
    };
    let policy = ip_acl::normalize_policy(&policy);
    if ip_acl::compile_policy(&policy).is_ok() {
        policy
    } else {
        RelayIpPolicy::default()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use super::*;
    use crate::{
        config::{NativeApi, NativeApiSource},
        keys::hash_client_key,
        relay_secrets::RelaySecretManager,
        standalone_config::{
            ClientKeyConfig, ContinuationPolicy, EndpointProvider, ModelRouteConfig,
            ModelRouteTargetConfig, ProviderEndpointConfig, RouteScope, RoutingStrategy,
            SettingConfig, StandaloneConfig, StandaloneConfigStore,
        },
    };

    fn manager() -> RelaySecretManager {
        RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager")
    }

    fn database_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-runtime-snapshot-{suffix}.sqlite"))
    }

    #[test]
    fn empty_snapshot_has_no_client_route() {
        assert_eq!(route_id_for_user(&StandaloneConfig::default(), 1), None);
    }

    #[tokio::test]
    async fn publishes_local_client_key_hash_prefix_route_and_policy() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let endpoint_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let secret = "local-client-secret".to_string();
        let snapshot = StandaloneConfig {
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "local endpoint".to_string(),
                provider: EndpointProvider::Generic,
                provider_region: None,
                base_url: "https://upstream.example".to_string(),
                native_api: NativeApi::Responses,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: "endpoint-secret".to_string(),
                api_keys: Vec::new(),
            }],
            routes: vec![ModelRouteConfig {
                rule_id: route_id,
                scope: RouteScope::Admin,
                owner_user_id: None,
                model_pattern: "*".to_string(),
                routing_strategy: RoutingStrategy::ClientKeyRendezvous,
                daily_max_requests: None,
                monthly_max_requests: None,
                enabled: true,
                targets: vec![ModelRouteTargetConfig {
                    target_id: Uuid::new_v4(),
                    endpoint_id,
                    position: 0,
                    enabled: true,
                    upstream_model: None,
                    responses_continuation_policy: ContinuationPolicy::ForceReplay,
                }],
            }],
            client_keys: vec![ClientKeyConfig {
                key_id: Uuid::new_v4(),
                user_id: 42,
                key_prefix: "pfy_local".to_string(),
                label: "local client".to_string(),
                secret: secret.clone(),
                enabled: true,
            }],
            settings: vec![SettingConfig {
                key: "relay_ip_whitelist".to_string(),
                version: 1,
                value: serde_json::json!({"allowed_cidrs": ["127.0.0.1/32"]}),
            }],
            ..StandaloneConfig::default()
        };
        let state = StandaloneRuntimeState::new(store.clone(), manager(), snapshot);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        state.set_bridge_sender("relay-key", Some(sender)).await;

        publish_snapshot(&state).await.expect("publish");
        let BridgeMessage::ConfigSnapshot(message) = receiver.recv().await.expect("snapshot")
        else {
            panic!("expected config snapshot");
        };
        assert_eq!(message.keys.len(), 1);
        assert_eq!(message.keys[0].key_hash, hash_client_key(&secret));
        assert_eq!(message.keys[0].key_prefix, "pfy_local");
        assert_eq!(message.keys[0].user_id, 42);
        assert_eq!(message.keys[0].route_id, endpoint_id.to_string());
        assert_eq!(message.relay_ip_policy.allowed_cidrs, vec!["127.0.0.1/32"]);

        let _ = std::fs::remove_file(path);
    }
}
