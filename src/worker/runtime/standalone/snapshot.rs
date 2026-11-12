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

    use super::*;
    use crate::standalone_config::StandaloneConfig;

    #[test]
    fn empty_snapshot_has_no_client_route() {
        assert_eq!(route_id_for_user(&StandaloneConfig::default(), 1), None);
    }
}
