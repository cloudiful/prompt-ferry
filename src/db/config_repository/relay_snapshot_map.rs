//! Mapping helpers for the bounded runtime snapshot published to relays.

use uuid::Uuid;

use crate::{
    bridge::protocol::ClientRoute, keys::hash_client_key, standalone_config::SettingConfig,
};

pub(super) fn policy_from_settings(
    snapshot: &[SettingConfig],
) -> crate::bridge::protocol::RelayIpPolicy {
    let Some(setting) = snapshot
        .iter()
        .find(|setting| setting.key == "relay_ip_whitelist")
    else {
        return crate::bridge::protocol::RelayIpPolicy::default();
    };
    serde_json::from_value(setting.value.clone()).unwrap_or_default()
}

pub(super) fn build_snapshot_keys_sqlite(
    snapshot: &crate::standalone_config::StandaloneConfig,
) -> Vec<ClientRoute> {
    let mut routes = Vec::new();
    for route in &snapshot.routes {
        for target in &route.targets {
            if !target.enabled {
                continue;
            }
            if !snapshot
                .endpoints
                .iter()
                .any(|endpoint| endpoint.endpoint_id == target.endpoint_id && endpoint.enabled)
            {
                continue;
            }
            routes.push((route.scope, route.owner_user_id, target.endpoint_id));
        }
    }
    if routes.is_empty()
        && let Some(endpoint) = snapshot.endpoints.iter().find(|endpoint| endpoint.enabled)
    {
        routes.push((
            crate::standalone_config::RouteScope::Admin,
            None,
            endpoint.endpoint_id,
        ));
    }
    let mut keys = Vec::new();
    for key in snapshot.client_keys.iter().filter(|key| key.enabled) {
        let Some(route_id) = pick_route_for_user(&routes, key.user_id) else {
            continue;
        };
        keys.push(ClientRoute {
            key_hash: hash_client_key(&key.secret),
            key_prefix: key.key_prefix.clone(),
            user_id: key.user_id,
            route_id: route_id.to_string(),
        });
    }
    keys
}

fn pick_route_for_user(
    routes: &[(crate::standalone_config::RouteScope, Option<i64>, Uuid)],
    user_id: i64,
) -> Option<Uuid> {
    routes
        .iter()
        .find(|(scope, owner, _)| {
            *scope == crate::standalone_config::RouteScope::User && *owner == Some(user_id)
        })
        .or_else(|| {
            routes
                .iter()
                .find(|(scope, _, _)| *scope == crate::standalone_config::RouteScope::Admin)
        })
        .map(|(_, _, endpoint_id)| *endpoint_id)
}
