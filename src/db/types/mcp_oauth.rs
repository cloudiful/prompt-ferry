//! Persisted personal MCP OAuth records (issue #1208 P2).
//!
//! Only non-secret status fields are ever exposed directly. Registration,
//! token, pending-authorization and pending-client material travel inside
//! [`crate::relay_secrets::EncryptedSecretEnvelope`] values that the storage
//! boundary encrypts and decrypts with the existing secret manager.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::McpServer;

/// The only server scope that may own personal OAuth credentials.
pub const MCP_OAUTH_SUPPORTED_SCOPE: &str = "user";
/// The auth mode that enables personal OAuth for a server.
pub const MCP_OAUTH_AUTH_MODE: &str = "oauth";
/// The only transport personal OAuth supports in the first version.
pub const MCP_OAUTH_SUPPORTED_TRANSPORT: &str = "http";
/// Single-use authorization state lifetime.
pub const MCP_OAUTH_STATE_TTL_SECONDS: i64 = 600;
/// Short refresh lease lifetime; no network work happens while holding it.
pub const MCP_OAUTH_LEASE_TTL_SECONDS: i64 = 60;
/// A freshly installed grant starts at generation one.
pub const MCP_OAUTH_INITIAL_GENERATION: i64 = 1;

/// Complete client registration and authorization-server metadata encrypted
/// at rest as one opaque engine snapshot.
///
/// The storage layer never interprets the snapshot; P3 deserializes the exact
/// rmcp types before use. Keeping one canonical snapshot avoids a second,
/// lossy representation of confidential method, redirect, resource or metadata.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct McpOAuthRegistration {
    pub snapshot: serde_json::Value,
}

/// Token material encrypted at rest.
///
/// `engine` is the complete serialized rmcp credential snapshot (token
/// response including vendor fields, receipt time and client metadata), while
/// `scopes`, `expires_at` and `issuer` are the only safe status projections.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct McpOAuthStoredCredentials {
    pub engine: serde_json::Value,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub issuer: Option<String>,
}

/// Pending authorization material encrypted at rest inside a state row.
///
/// `snapshot` is the complete serialized rmcp `StoredAuthorizationState`; the
/// redirect URI remains in the independently validated state binding instead
/// of being duplicated here.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct McpOAuthAuthorizationState {
    pub snapshot: serde_json::Value,
}

impl std::fmt::Debug for McpOAuthRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpOAuthRegistration(<redacted>)")
    }
}

impl std::fmt::Debug for McpOAuthStoredCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthStoredCredentials")
            .field("engine", &"<redacted>")
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .field("issuer", &self.issuer)
            .finish()
    }
}

impl std::fmt::Debug for McpOAuthAuthorizationState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpOAuthAuthorizationState(<redacted>)")
    }
}

/// Public status of one stored credential. It never carries a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthCredential {
    pub server_id: Uuid,
    pub owner_user_id: i64,
    pub connection_id: Uuid,
    pub generation: i64,
    pub config_fingerprint: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub issuer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Decrypted credential bundle for the OAuth engine.
#[derive(Clone, PartialEq)]
pub struct McpOAuthCredentialSecrets {
    pub credential: McpOAuthCredential,
    pub registration: McpOAuthRegistration,
    pub credentials: McpOAuthStoredCredentials,
}

/// A fresh callback grant: it installs a new connection epoch.
#[derive(Clone, PartialEq)]
pub struct McpOAuthGrant {
    pub registration: McpOAuthRegistration,
    pub credentials: McpOAuthStoredCredentials,
}

impl std::fmt::Debug for McpOAuthCredentialSecrets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthCredentialSecrets")
            .field("credential", &self.credential)
            .field("registration", &"<redacted>")
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for McpOAuthGrant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpOAuthGrant(<redacted>)")
    }
}

/// Fence required before a refresh may overwrite stored credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpOAuthRefreshFence {
    pub connection_id: Uuid,
    pub generation: i64,
    pub lease_owner: Uuid,
}

/// Non-secret binding carried by one authorization state row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthStateBinding {
    pub server_id: Uuid,
    pub owner_user_id: i64,
    pub session_binding_hash: String,
    pub connection_id: Uuid,
    pub config_fingerprint: String,
    pub redirect_uri: String,
    pub expires_at: DateTime<Utc>,
}

/// One single-use authorization state write.
#[derive(Clone, PartialEq)]
pub struct McpOAuthStateInput {
    pub state_key_hash: String,
    pub session_binding_hash: String,
    pub server_id: Uuid,
    pub owner_user_id: i64,
    pub connection_id: Uuid,
    pub config_fingerprint: String,
    pub redirect_uri: String,
    pub expires_at: DateTime<Utc>,
    pub authorization: McpOAuthAuthorizationState,
    pub pending_client: Option<McpOAuthRegistration>,
}

/// The decrypted result of atomically consuming one state row.
#[derive(Clone, PartialEq)]
pub struct McpOAuthConsumedState {
    pub binding: McpOAuthStateBinding,
    pub authorization: McpOAuthAuthorizationState,
    pub pending_client: Option<McpOAuthRegistration>,
}

impl std::fmt::Debug for McpOAuthStateInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthStateInput")
            .field("state_key_hash", &self.state_key_hash)
            .field("session_binding_hash", &self.session_binding_hash)
            .field("server_id", &self.server_id)
            .field("owner_user_id", &self.owner_user_id)
            .field("connection_id", &self.connection_id)
            .field("config_fingerprint", &self.config_fingerprint)
            .field("redirect_uri", &self.redirect_uri)
            .field("expires_at", &self.expires_at)
            .field("authorization", &"<redacted>")
            .field("pending_client", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for McpOAuthConsumedState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthConsumedState")
            .field("binding", &self.binding)
            .field("authorization", &"<redacted>")
            .field("pending_client", &"<redacted>")
            .finish()
    }
}

/// One short conditional refresh lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthLease {
    pub server_id: Uuid,
    pub owner_user_id: i64,
    pub connection_id: Uuid,
    pub generation: i64,
    pub lease_owner: Uuid,
    pub expires_at: DateTime<Utc>,
}

/// A stable fingerprint of every server field whose change revokes OAuth.
///
/// The trigger deletes OAuth rows when any of these fields changes, so the
/// fingerprint and the trigger must cover exactly the same set: url, auth
/// mode, scope, owner, transport and proxy. Name and timeout edits keep it
/// stable on purpose.
pub fn config_fingerprint(server: &McpServer) -> String {
    let mut hasher = Sha256::new();
    for part in [
        server.url.as_deref().unwrap_or_default(),
        server.auth_mode.as_str(),
        server.scope.as_str(),
        server.transport.as_str(),
        server.proxy_url.as_deref().unwrap_or_default(),
    ] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    match server.owner_user_id {
        Some(owner) => {
            hasher.update([1_u8]);
            hasher.update(owner.to_le_bytes());
        }
        None => hasher.update([0_u8]),
    }
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Verify that a server may own personal OAuth for `owner_user_id`.
///
/// Returns the current config fingerprint so callers can store or compare it.
pub fn authorize_mcp_oauth_server(
    server: &McpServer,
    owner_user_id: i64,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        server.scope == MCP_OAUTH_SUPPORTED_SCOPE,
        "personal MCP OAuth requires a user-scoped server"
    );
    anyhow::ensure!(
        server.owner_user_id == Some(owner_user_id),
        "personal MCP OAuth requires the server owner"
    );
    anyhow::ensure!(
        server.auth_mode == MCP_OAUTH_AUTH_MODE,
        "personal MCP OAuth requires oauth auth mode"
    );
    anyhow::ensure!(
        server.transport == MCP_OAUTH_SUPPORTED_TRANSPORT,
        "personal MCP OAuth supports only the http transport"
    );
    Ok(config_fingerprint(server))
}

/// Render a scope list for storage; the status column stays non-secret.
pub fn join_scopes(scopes: &[String]) -> String {
    scopes.join(" ")
}

/// Parse a stored scope list.
pub fn parse_scopes(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}
