//! SQLite (standalone) configuration snapshot reader.
//!
//! Reuses `StandaloneConfigStore::load_snapshot`, which already decrypts the
//! envelope-wrapped secrets, then adds the domains that live outside the
//! bundled snapshot (MCP servers and credentials, per-endpoint OpenAI admin
//! key and ChatGPT OAuth token). Users come from the shared user store so both
//! backends export identical user records.

use anyhow::{Context, Result};

use super::snapshot::{
    ClientKeySnapshot, EndpointApiKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot,
    McpCredentialSnapshot, McpServerSnapshot, ModelRouteSnapshot, ModelRouteTargetSnapshot,
    RelaySnapshot, SecretRecovery, SettingSnapshot, SnapshotDomains, UserSnapshot,
};
use crate::{
    config::NativeApi,
    db::config_repository::{ConfigRepository, SqliteConfigRepository},
    standalone_config::{
        ManagedRelayConfig, ModelRouteConfig, ProviderEndpointConfig, StandaloneConfig,
    },
};

pub(super) async fn read(
    repository: &ConfigRepository,
    users: Vec<UserSnapshot>,
) -> Result<SnapshotDomains> {
    let sqlite = repository
        .as_sqlite()
        .context("sqlite configuration snapshot requires a sqlite repository")?;
    let snapshot = sqlite
        .store()
        .load_snapshot(sqlite.manager())
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let endpoints = read_endpoints(sqlite, &snapshot).await?;
    let mcp_servers = sqlite
        .store()
        .list_mcp_servers(sqlite.manager())
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let mcp_credentials = read_mcp_credentials(repository).await?;

    Ok(SnapshotDomains {
        users,
        client_keys: snapshot
            .client_keys
            .iter()
            .map(convert_client_key)
            .collect(),
        endpoints,
        model_routes: snapshot.routes.iter().map(convert_route).collect(),
        mcp_servers: mcp_servers.into_iter().map(convert_mcp_server).collect(),
        mcp_credentials,
        relays: snapshot.relays.iter().map(convert_relay).collect(),
        settings: snapshot.settings.iter().map(convert_setting).collect(),
        // Per-user redaction rules and the raw object store are
        // PostgreSQL-only configuration domains.
        user_redaction_configs: Vec::new(),
        raw_object_store: None,
    })
}

async fn read_endpoints(
    sqlite: &SqliteConfigRepository,
    snapshot: &StandaloneConfig,
) -> Result<Vec<EndpointSnapshot>> {
    let mut endpoints = Vec::new();
    for endpoint in &snapshot.endpoints {
        let store = sqlite.store();
        let manager = sqlite.manager();
        let oauth = store
            .get_endpoint_oauth_token(manager, endpoint.endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
            .map(|token| EndpointOAuthSnapshot {
                access_token: token.access_token,
                refresh_token: token.refresh_token,
                expires_at: token.expires_at,
            });
        let admin_api_key = store
            .endpoint_admin_api_key(manager, endpoint.endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        endpoints.push(convert_endpoint(endpoint, oauth, admin_api_key));
    }
    Ok(endpoints)
}

fn convert_client_key(key: &crate::standalone_config::ClientKeyConfig) -> ClientKeySnapshot {
    ClientKeySnapshot {
        key_id: key.key_id,
        user_id: key.user_id,
        key_prefix: key.key_prefix.clone(),
        label: key.label.clone(),
        enabled: key.enabled,
        // The standalone store exposes neither a per-key timestamp nor the
        // scalar hash, so the record carries only what it can prove.
        created_at: None,
        secret_state: SecretRecovery::Recovered,
        secret: Some(key.secret.clone()),
        key_hash: Some(crate::keys::hash_client_key(&key.secret)),
    }
}

fn convert_endpoint(
    endpoint: &ProviderEndpointConfig,
    oauth: Option<EndpointOAuthSnapshot>,
    admin_api_key: Option<String>,
) -> EndpointSnapshot {
    let api_keys = endpoint
        .api_keys
        .iter()
        .map(|key| EndpointApiKeySnapshot {
            key_id: key.key_id,
            key_label: key.key_label.clone(),
            position: key.position,
            enabled: key.enabled,
            api_key: non_empty(Some(key.api_key.clone())),
        })
        .collect();
    EndpointSnapshot {
        endpoint_id: endpoint.endpoint_id,
        // The standalone runtime only stores administrator-owned endpoints.
        scope: "admin".to_string(),
        owner_user_id: None,
        name: endpoint.name.clone(),
        provider: provider_name(endpoint).to_string(),
        provider_region: endpoint
            .provider_region
            .map(|region| region_name(region).to_string()),
        service_tier: endpoint.service_tier.clone(),
        base_url: endpoint.base_url.clone(),
        native_api: endpoint.native_api.as_str().to_string(),
        native_api_source: endpoint.native_api_source.as_str().to_string(),
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        active_windows: non_empty(endpoint.active_windows.clone()),
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        api_key: non_empty(Some(endpoint.api_key.clone())),
        api_keys,
        proxy_url: non_empty(endpoint.proxy_url.clone()),
        admin_api_key,
        oauth,
    }
}

fn convert_route(route: &ModelRouteConfig) -> ModelRouteSnapshot {
    ModelRouteSnapshot {
        rule_id: route.rule_id,
        scope: route.scope.as_str().to_string(),
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern.clone(),
        routing_strategy: route.routing_strategy.as_str().to_string(),
        enabled: route.enabled,
        // The standalone route model carries no timestamps.
        created_at: None,
        updated_at: None,
        targets: route
            .targets
            .iter()
            .map(|target| ModelRouteTargetSnapshot {
                target_id: target.target_id,
                endpoint_id: target.endpoint_id,
                position: target.position,
                enabled: target.enabled,
                upstream_model: target.upstream_model.clone(),
                native_api: native_api_name(target.native_api).to_string(),
                proxy_url_override: non_empty(target.proxy_url_override.clone()),
                active_windows: non_empty(target.active_windows.clone()),
                dev_system_normalize: target.dev_system_normalize,
                thinking_downgrade_enabled: target.thinking_downgrade_enabled,
                thinking_effort_override: target.thinking_effort_override.clone(),
                compact_mode: target.compact_mode.clone(),
                service_tier: target.service_tier.clone(),
            })
            .collect(),
    }
}

fn convert_relay(relay: &ManagedRelayConfig) -> RelaySnapshot {
    RelaySnapshot {
        relay_id: relay.relay_id,
        name: relay.name.clone(),
        relay_url: relay.relay_url.clone(),
        enabled: relay.enabled,
        tls_mode: relay.tls_mode.as_str().to_string(),
        bridge_encryption_mode: relay.bridge_encryption_mode.as_str().to_string(),
        relay_ca_pem: non_empty(relay.relay_ca_pem.clone()),
        client_cert_pem: non_empty(relay.client_cert_pem.clone()),
        client_key_pem: non_empty(relay.client_key_pem.clone()),
        bridge_encryption_key: non_empty(relay.bridge_encryption_key.clone()),
    }
}

fn convert_setting(setting: &crate::standalone_config::SettingConfig) -> SettingSnapshot {
    SettingSnapshot {
        key: setting.key.clone(),
        version: setting.version,
        value: setting.value.clone(),
        updated_at: None,
    }
}

fn convert_mcp_server(server: crate::db::McpServer) -> McpServerSnapshot {
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

async fn read_mcp_credentials(repository: &ConfigRepository) -> Result<Vec<McpCredentialSnapshot>> {
    let mut credentials = Vec::new();
    for credential in repository.list_all_mcp_credentials().await? {
        credentials.push(McpCredentialSnapshot {
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
        });
    }
    Ok(credentials)
}

fn provider_name(endpoint: &ProviderEndpointConfig) -> &'static str {
    use crate::standalone_config::EndpointProvider as Provider;
    match endpoint.provider {
        Provider::Minimax => "minimax",
        Provider::CommandCode => "command_code",
        Provider::OpencodeGo => "opencode_go",
        Provider::OpenRouter => "openrouter",
        Provider::Glm => "glm",
        Provider::DeepSeek => "deepseek",
        Provider::OpenAi => "openai",
        Provider::Generic => "generic",
    }
}

fn region_name(region: crate::standalone_config::EndpointRegion) -> &'static str {
    use crate::standalone_config::EndpointRegion as Region;
    match region {
        Region::Cn => "cn",
        Region::Global => "global",
    }
}

fn native_api_name(api: NativeApi) -> &'static str {
    api.as_str()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
