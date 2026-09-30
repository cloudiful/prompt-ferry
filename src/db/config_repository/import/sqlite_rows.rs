//! SQLite import rows that live outside the `StandaloneConfig` shape.
//!
//! Users, MCP servers (with their rebuilt bearer tokens), per-endpoint OAuth
//! tokens, and the OpenAI Admin API key each have their own store table, so
//! they are converted here and handed to the same transaction alongside the
//! bundled snapshot.

use std::collections::HashMap;

use anyhow::Result;

use super::super::snapshot::{McpCredentialSnapshot, McpServerSnapshot, SnapshotDomains};
use super::ImportError;
use crate::db::{McpBearerToken, McpServerInput};
use crate::standalone_config::{
    ImportEndpointAdminKey, ImportEndpointOAuth, ImportMcpServer, ImportUser,
};

pub(super) fn build_users(domains: &SnapshotDomains) -> Result<Vec<ImportUser>, ImportError> {
    domains
        .users
        .iter()
        .map(|user| {
            let password_hash = user
                .password_hash
                .clone()
                .ok_or(ImportError::InvalidArchive(
                    "a user has no password hash to restore",
                ))?;
            Ok(ImportUser {
                user_id: user.user_id,
                login_name: user.login_name.clone(),
                display_name: user.display_name.clone(),
                password_hash,
                is_admin: user.is_admin,
                is_active: user.is_active,
                created_at: user.created_at,
                updated_at: user.updated_at,
            })
        })
        .collect()
}

pub(super) fn build_mcp_servers(
    domains: &SnapshotDomains,
) -> Result<Vec<ImportMcpServer>, ImportError> {
    let mut credentials: HashMap<uuid::Uuid, Vec<&McpCredentialSnapshot>> = HashMap::new();
    for credential in &domains.mcp_credentials {
        credentials
            .entry(credential.server_id)
            .or_default()
            .push(credential);
    }
    domains
        .mcp_servers
        .iter()
        .map(|server| {
            let tokens = credentials.remove(&server.server_id).unwrap_or_default();
            let input = convert_mcp_server(server, tokens);
            Ok(ImportMcpServer {
                server_id: server.server_id,
                created_at: server.created_at,
                updated_at: server.updated_at,
                input,
            })
        })
        .collect()
}

/// Rebuild `bearer_tokens_json` from the credential rows. The export derives
/// one credential per bearer token, so ordering by `position` restores the
/// original array (label, position, and enabled flag included).
fn convert_mcp_server(
    server: &McpServerSnapshot,
    tokens: Vec<&McpCredentialSnapshot>,
) -> McpServerInput {
    let mut tokens = tokens;
    tokens.sort_by_key(|credential| credential.position);
    let bearer_tokens: Vec<serde_json::Value> = tokens
        .iter()
        .map(|credential| {
            serde_json::to_value(McpBearerToken {
                token: credential.secret.clone(),
                enabled: credential.enabled,
            })
            .unwrap_or(serde_json::Value::Null)
        })
        .collect();
    McpServerInput {
        scope: server.scope.clone(),
        owner_user_id: server.owner_user_id,
        source_endpoint_id: server.source_endpoint_id,
        name: server.name.clone(),
        aggregate_naming_mode: server.aggregate_naming_mode.clone(),
        transport: server.transport.clone(),
        provider_kind: server.provider_kind.clone(),
        url: server.url.clone(),
        command: server.command.clone(),
        args: server.args.clone(),
        env_json: server.env_json.clone(),
        bearer_tokens_json: serde_json::Value::Array(bearer_tokens),
        http_headers_json: server.http_headers_json.clone(),
        auth_mode: server.auth_mode.clone(),
        basic_username: server.basic_username.clone(),
        basic_password: server.basic_password.clone(),
        proxy_url: server.proxy_url.clone(),
        tool_filter_mode: server.tool_filter_mode.clone(),
        allowed_tools: server.allowed_tools.clone(),
        disabled_tools: server.disabled_tools.clone(),
        disabled_resources: server.disabled_resources.clone(),
        enabled: server.enabled,
        timeout_ms: server.timeout_ms,
        lifecycle_policy: server.lifecycle_policy.clone(),
        lifecycle_manual_protocol_version: server.lifecycle_manual_protocol_version.clone(),
    }
}

pub(super) fn build_oauth_tokens(
    domains: &SnapshotDomains,
) -> Result<Vec<ImportEndpointOAuth>, ImportError> {
    let mut tokens = Vec::new();
    for endpoint in &domains.endpoints {
        if let Some(oauth) = &endpoint.oauth {
            tokens.push(ImportEndpointOAuth {
                endpoint_id: endpoint.endpoint_id,
                access_token: oauth.access_token.clone(),
                refresh_token: oauth.refresh_token.clone(),
                expires_at: oauth.expires_at,
            });
        }
    }
    Ok(tokens)
}

pub(super) fn build_admin_keys(
    domains: &SnapshotDomains,
) -> Result<Vec<ImportEndpointAdminKey>, ImportError> {
    Ok(domains
        .endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint
                .admin_api_key
                .as_ref()
                .map(|api_key| ImportEndpointAdminKey {
                    endpoint_id: endpoint.endpoint_id,
                    api_key: api_key.clone(),
                })
        })
        .collect())
}
