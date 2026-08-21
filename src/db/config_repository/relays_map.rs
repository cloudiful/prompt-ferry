//! Mapping helpers between the unified managed relay DTO and the
//! PostgreSQL/SQLite row models, plus the snapshot publication glue.

use anyhow::Result;
use uuid::Uuid;

use crate::{
    bridge::protocol::{ClientRoute, RelayIpPolicy},
    config::{BridgeEncryptionMode, TlsMode},
    db::{ManagedRelayInput, ManagedRelayRow as PgManagedRelayRow},
    keys::hash_client_key,
    relay_secrets::RelaySecretManager,
    standalone_config::{ManagedRelayConfig as ScManagedRelay, SettingConfig},
};

use super::{ManagedRelaySecrets, UnifiedManagedRelay};

pub(super) fn from_pg_row(row: PgManagedRelayRow) -> UnifiedManagedRelay {
    let has_relay_ca = row.relay_ca_envelope().is_some();
    let has_client_cert = row.client_cert_envelope().is_some();
    let has_client_key = row.client_key_envelope().is_some();
    let has_bridge_key = row.bridge_encryption_key_envelope().is_some();
    let relay_url = row.relay_url.clone();
    let enabled = row.enabled;
    let relay_id = row.relay_id;
    let name = row.name.clone();
    UnifiedManagedRelay {
        relay_id,
        name,
        relay_url,
        enabled,
        tls_mode: row.tls_mode(),
        bridge_encryption_mode: row.bridge_encryption_mode(),
        has_relay_ca,
        has_client_cert,
        has_client_key,
        has_bridge_key,
    }
}

pub(super) fn from_sc(relay: ScManagedRelay) -> UnifiedManagedRelay {
    UnifiedManagedRelay {
        relay_id: relay.relay_id,
        name: relay.name,
        relay_url: relay.relay_url,
        enabled: relay.enabled,
        tls_mode: relay.tls_mode,
        bridge_encryption_mode: relay.bridge_encryption_mode,
        has_relay_ca: relay.relay_ca_pem.is_some(),
        has_client_cert: relay.client_cert_pem.is_some(),
        has_client_key: relay.client_key_pem.is_some(),
        has_bridge_key: relay.bridge_encryption_key.is_some(),
    }
}

pub(super) fn decrypt_envelope(
    manager: &RelaySecretManager,
    envelope: Option<&crate::relay_secrets::EncryptedSecretEnvelope>,
) -> Result<Option<String>> {
    match envelope {
        None => Ok(None),
        Some(envelope) => Ok(Some(manager.decrypt(envelope)?)),
    }
}

pub(super) fn sqlite_relay_from_input(
    input: ManagedRelayInput,
    manager: &RelaySecretManager,
) -> Result<ScManagedRelay> {
    Ok(ScManagedRelay {
        relay_id: Uuid::new_v4(),
        name: input.name,
        relay_url: input.relay_url,
        enabled: input.enabled,
        tls_mode: input.tls_mode,
        bridge_encryption_mode: input.bridge_encryption_mode,
        relay_ca_pem: decrypt_envelope(manager, input.relay_ca.as_ref())?,
        client_cert_pem: decrypt_envelope(manager, input.client_cert.as_ref())?,
        client_key_pem: decrypt_envelope(manager, input.client_key.as_ref())?,
        bridge_encryption_key: decrypt_envelope(manager, input.bridge_encryption_key.as_ref())?,
    })
}

pub(super) fn managed_secrets_from_row(row: &PgManagedRelayRow) -> ManagedRelaySecrets {
    ManagedRelaySecrets {
        relay_ca: row.relay_ca_envelope(),
        client_cert: row.client_cert_envelope(),
        client_key: row.client_key_envelope(),
        bridge_key: row.bridge_encryption_key_envelope(),
    }
}

pub(super) fn tls_mode_value(mode: TlsMode) -> &'static str {
    mode.as_str()
}

pub(super) fn bridge_mode_value(mode: BridgeEncryptionMode) -> &'static str {
    mode.as_str()
}

pub(super) fn policy_from_settings(snapshot: &[SettingConfig]) -> RelayIpPolicy {
    let Some(setting) = snapshot
        .iter()
        .find(|setting| setting.key == "relay_ip_whitelist")
    else {
        return RelayIpPolicy::default();
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
    if routes.is_empty() {
        if let Some(endpoint) = snapshot.endpoints.iter().find(|endpoint| endpoint.enabled) {
            routes.push((
                crate::standalone_config::RouteScope::Admin,
                None,
                endpoint.endpoint_id,
            ));
        }
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
