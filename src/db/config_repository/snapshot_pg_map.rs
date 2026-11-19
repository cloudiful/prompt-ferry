//! Row-to-snapshot mapping for the PostgreSQL configuration export.
//!
//! Pure conversions only: each function turns one already-loaded PostgreSQL
//! row (or row collection) into its snapshot record, normalizing blank
//! secret-shaped strings to `None` so a record never claims an empty secret.
//! The reads that feed these functions live in [`super::snapshot_postgres`].

use super::snapshot::{
    ClientKeySnapshot, EndpointApiKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot,
    McpCredentialSnapshot, McpServerSnapshot, ModelRouteSnapshot, ModelRouteTargetSnapshot,
    SecretRecovery,
};
use crate::db::McpOAuthGrantSnapshot;
use crate::db::{
    ClientKey as PgClientKey, EndpointApiKey, McpCredential, McpServer, ModelEndpointRule,
    ProviderEndpoint,
};

pub(super) fn convert_client_key(key: PgClientKey) -> ClientKeySnapshot {
    let secret = non_empty(key.secret);
    // The lookup hash is a pure function of the key material, so a recovered
    // secret yields the hash the target backend needs. A legacy row whose
    // plaintext column was never populated has neither and exports as
    // unrecoverable.
    let key_hash = secret.as_deref().map(crate::keys::hash_client_key);
    let secret_state = match (&secret, &key_hash) {
        (Some(_), _) => SecretRecovery::Recovered,
        (None, Some(_)) => SecretRecovery::HashOnly,
        (None, None) => SecretRecovery::Absent,
    };
    ClientKeySnapshot {
        key_id: uuid::Uuid::from_u64_pair(key.key_id as u64, 0),
        user_id: key.user_id,
        key_prefix: key.key_prefix,
        label: key.label,
        enabled: key.enabled,
        created_at: Some(key.created_at),
        secret_state,
        secret,
        key_hash,
    }
}

/// Project one stored endpoint and its persisted API-key rows.
///
/// `persisted_api_keys` must be the rows stored for this endpoint, never the
/// runtime list: the runtime list is hydrated with a virtual key when a legacy
/// endpoint carries only the primary `api_key`, and that fabricated identity
/// must not travel as if it were a database row. Every persisted row is mapped
/// unchanged, including a real stored nil UUID.
pub(super) fn convert_endpoint(
    endpoint: ProviderEndpoint,
    admin_api_key: Option<String>,
    oauth: Option<EndpointOAuthSnapshot>,
    persisted_api_keys: Vec<EndpointApiKey>,
) -> EndpointSnapshot {
    let api_key = non_empty(Some(endpoint.api_key.clone()));
    let api_keys = persisted_api_keys
        .into_iter()
        .map(|key| EndpointApiKeySnapshot {
            key_id: key.key_id,
            key_label: key.key_label,
            position: key.position,
            enabled: key.enabled,
            api_key: non_empty(Some(key.api_key)),
        })
        .collect();
    EndpointSnapshot {
        endpoint_id: endpoint.endpoint_id,
        scope: endpoint.scope,
        owner_user_id: endpoint.owner_user_id,
        name: endpoint.name,
        provider: endpoint.provider.as_str().to_string(),
        provider_region: endpoint
            .provider_region
            .map(|region| region.as_str().to_string()),
        service_tier: endpoint.service_tier,
        base_url: endpoint.base_url,
        native_api: endpoint.native_api,
        native_api_source: endpoint.native_api_source,
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        active_windows: crate::db::storage_value(&endpoint.active_windows),
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        api_key,
        api_keys,
        proxy_url: non_empty(endpoint.proxy_url),
        admin_api_key,
        oauth,
    }
}

pub(super) fn convert_route(route: ModelEndpointRule) -> ModelRouteSnapshot {
    ModelRouteSnapshot {
        rule_id: route.rule_id,
        scope: route.scope,
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern,
        routing_strategy: route.routing_strategy.as_str().to_string(),
        enabled: route.enabled,
        created_at: Some(route.created_at),
        updated_at: Some(route.updated_at),
        targets: route
            .targets
            .into_iter()
            .map(|target| ModelRouteTargetSnapshot {
                target_id: target.target_id,
                endpoint_id: target.endpoint_id,
                position: target.position,
                enabled: target.enabled,
                upstream_model: target.upstream_model,
                native_api: target.native_api.as_str().to_string(),
                proxy_url_override: non_empty(target.proxy_url_override),
                active_windows: crate::db::storage_value(&target.active_windows),
                dev_system_normalize: target.dev_system_normalize,
                thinking_downgrade_enabled: target.thinking_downgrade_enabled,
                thinking_effort_override: target.thinking_effort_override,
                compact_mode: target.compact_mode.as_str().to_string(),
                service_tier: target.service_tier,
                embedding_dimensions_override: target.embedding_dimensions_override,
                prefer_new_sessions: target.prefer_new_sessions,
            })
            .collect(),
    }
}

pub(super) fn convert_mcp_server(server: McpServer) -> McpServerSnapshot {
    McpServerSnapshot {
        server_id: server.server_id,
        source_endpoint_id: server.source_endpoint_id,
        scope: server.scope,
        owner_user_id: server.owner_user_id,
        name: server.name,
        aggregate_naming_mode: server.aggregate_naming_mode,
        transport: server.transport,
        provider_kind: server.provider_kind,
        url: server.url,
        command: server.command,
        args: server.args,
        env_json: server.env_json,
        http_headers_json: server.http_headers_json,
        auth_mode: server.auth_mode,
        basic_username: server.basic_username,
        basic_password: non_empty(server.basic_password),
        proxy_url: non_empty(server.proxy_url),
        tool_filter_mode: server.tool_filter_mode,
        allowed_tools: server.allowed_tools,
        disabled_tools: server.disabled_tools,
        disabled_resources: server.disabled_resources,
        enabled: server.enabled,
        timeout_ms: server.timeout_ms,
        lifecycle_policy: server.lifecycle_policy,
        lifecycle_manual_protocol_version: server.lifecycle_manual_protocol_version,
        created_at: server.created_at,
        updated_at: server.updated_at,
    }
}

pub(super) fn convert_mcp_credential(credential: McpCredential) -> McpCredentialSnapshot {
    McpCredentialSnapshot {
        credential_id: credential.credential_id,
        server_id: credential.server_id,
        credential_label: credential.credential_label,
        secret: credential.secret,
        position: credential.position,
        enabled: credential.enabled,
        provider_kind: credential.provider_kind,
        created_at: credential.created_at,
        updated_at: credential.updated_at,
    }
}

/// Project one decrypted personal MCP OAuth grant onto its portable record.
///
/// The connection epoch and the generation are dropped on purpose: the
/// destination installs a fresh epoch, and the archived fingerprint travels
/// unchanged so the restored server keeps its compatibility check.
pub(super) fn convert_mcp_oauth_grant(
    secrets: crate::db::McpOAuthCredentialSecrets,
) -> McpOAuthGrantSnapshot {
    McpOAuthGrantSnapshot {
        server_id: secrets.credential.server_id,
        owner_user_id: secrets.credential.owner_user_id,
        config_fingerprint: secrets.credential.config_fingerprint,
        registration: secrets.registration,
        credentials: secrets.credentials,
    }
}

pub(super) fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::ProviderEndpointRow;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn stamp(second: u32) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + i64::from(second), 0)
            .single()
            .expect("timestamp")
    }

    fn endpoint(name: &str) -> ProviderEndpoint {
        ProviderEndpoint::from(ProviderEndpointRow {
            endpoint_id: Uuid::from_u64_pair(1, 0),
            scope: "admin".to_string(),
            owner_user_id: None,
            name: name.to_string(),
            provider: "generic".to_string(),
            provider_region: None,
            service_tier: None,
            base_url: "http://127.0.0.1:1".to_string(),
            native_api: "chat".to_string(),
            native_api_source: "manual".to_string(),
            api_key: "primary-secret".to_string(),
            proxy_url: None,
            active_windows: None,
            key_lb_enabled: false,
            enabled: true,
            mcp_enabled: false,
            created_at: stamp(0),
            updated_at: stamp(0),
        })
    }

    fn key(key_id: Uuid, label: &str, position: i32, enabled: bool) -> EndpointApiKey {
        EndpointApiKey {
            key_id,
            endpoint_id: Uuid::from_u64_pair(1, 0),
            key_label: label.to_string(),
            api_key: format!("secret-{label}"),
            position,
            enabled,
            created_at: stamp(position as u32),
            updated_at: stamp(position as u32),
        }
    }

    #[test]
    fn a_runtime_virtual_key_is_not_exported() {
        let virtual_key = key(Uuid::nil(), "virtual", 0, true);
        let mut source = endpoint("legacy-only");
        source.api_keys = vec![virtual_key];
        let snapshot = convert_endpoint(source, None, None, Vec::new());
        assert!(snapshot.api_keys.is_empty());
        assert_eq!(snapshot.api_key.as_deref(), Some("primary-secret"));
    }

    #[test]
    fn a_persisted_nil_key_keeps_its_metadata() {
        let stored = key(Uuid::nil(), "legacy-nil", 7, false);
        let snapshot = convert_endpoint(endpoint("legacy-nil"), None, None, vec![stored]);
        assert_eq!(snapshot.api_keys.len(), 1);
        let exported = &snapshot.api_keys[0];
        assert_eq!(exported.key_id, Uuid::nil());
        assert_eq!(exported.key_label, "legacy-nil");
        assert_eq!(exported.position, 7);
        assert!(!exported.enabled);
        assert_eq!(exported.api_key.as_deref(), Some("secret-legacy-nil"));
    }

    #[test]
    fn persisted_keys_keep_their_metadata_and_order() {
        let first = key(Uuid::from_u64_pair(9, 0), "first", 0, true);
        let second = key(Uuid::from_u64_pair(10, 0), "second", 1, false);
        let snapshot = convert_endpoint(
            endpoint("multi"),
            None,
            None,
            vec![second.clone(), first.clone()],
        );
        assert_eq!(snapshot.api_keys.len(), 2);
        assert_eq!(snapshot.api_keys[0].key_id, second.key_id);
        assert_eq!(snapshot.api_keys[1].key_id, first.key_id);
        assert_eq!(snapshot.api_keys[0].position, second.position);
        assert!(!snapshot.api_keys[0].enabled);
        assert_eq!(snapshot.api_keys[1].key_label, "first");
    }
}
