//! Row-to-snapshot mapping for the PostgreSQL configuration export.
//!
//! Pure conversions only: each function turns one already-loaded PostgreSQL
//! row (or row collection) into its snapshot record, normalizing blank
//! secret-shaped strings to `None` so a record never claims an empty secret.
//! The reads that feed these functions live in [`super::snapshot_postgres`].

use super::ManagedRelaySecrets;
use super::snapshot::{
    ClientKeySnapshot, EndpointApiKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot,
    McpCredentialSnapshot, McpServerSnapshot, ModelRouteSnapshot, ModelRouteTargetSnapshot,
    RelaySnapshot, SecretRecovery,
};
use crate::db::{
    ClientKey as PgClientKey, ManagedRelayRow, McpCredential, McpServer, ModelEndpointRule,
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

pub(super) fn convert_endpoint(
    endpoint: ProviderEndpoint,
    admin_api_key: Option<String>,
    oauth: Option<EndpointOAuthSnapshot>,
) -> EndpointSnapshot {
    let api_key = non_empty(Some(endpoint.api_key.clone()));
    let api_keys = endpoint
        .api_keys
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
        default_cost: credential.default_cost,
        strict_mode: credential.strict_mode,
        billing_period_start: credential.billing_period_start,
        billing_period_end: credential.billing_period_end,
        created_at: credential.created_at,
        updated_at: credential.updated_at,
    }
}

/// A managed relay row plus its sealed secrets, decrypted for the archive.
pub(super) fn convert_relay(
    row: ManagedRelayRow,
    secrets: ManagedRelaySecrets,
    manager: Option<&crate::relay_secrets::RelaySecretManager>,
) -> anyhow::Result<RelaySnapshot> {
    let decrypt = |envelope: Option<crate::relay_secrets::EncryptedSecretEnvelope>| match envelope {
        Some(envelope) => match manager {
            Some(manager) => Ok(Some(manager.decrypt(&envelope)?)),
            None => anyhow::bail!(
                "managed relay secrets are sealed but no worker configuration \
                 encryption key is configured"
            ),
        },
        None => Ok(None),
    };
    // Read the mode strings before the row is destructured, so the snapshot
    // carries the stored values rather than a re-derived default.
    let tls_mode = row.tls_mode().as_str().to_string();
    let bridge_encryption_mode = row.bridge_encryption_mode().as_str().to_string();
    Ok(RelaySnapshot {
        relay_id: row.relay_id,
        name: row.name,
        relay_url: row.relay_url,
        enabled: row.enabled,
        tls_mode,
        bridge_encryption_mode,
        relay_ca_pem: decrypt(secrets.relay_ca)?,
        client_cert_pem: decrypt(secrets.client_cert)?,
        client_key_pem: decrypt(secrets.client_key)?,
        bridge_encryption_key: decrypt(secrets.bridge_key)?,
    })
}

pub(super) fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
