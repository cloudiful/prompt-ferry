//! Shared fixtures for the configuration snapshot/archive tests.
//!
//! [`sample_snapshot`] builds the smallest in-memory snapshot that still covers
//! a user record (with an Argon2id hash) and a setting, so the archive codec
//! tests never touch a database.

use crate::db::config_repository::snapshot::{
    ConfigBackendKind, ConfigSnapshot, SettingSnapshot, SnapshotDomains, UserSnapshot,
};
use crate::db::config_repository::snapshot_source::empty_domains;

pub(super) const PASSPHRASE: &str = "correct horse battery staple";

/// The domain JSON a legacy archive carried, frozen before the personal MCP
/// OAuth domain existed. The bytes are the canonical serialization of the
/// legacy domain records, so the fingerprint below is the one that archive
/// carried.
pub(super) const LEGACY_DOMAINS_JSON: &str = r#"{"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:20Z","updated_at":"2023-11-14T22:13:20Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[],"settings":[{"key":"redaction_enabled","version":1,"value":true,"updated_at":null}],"user_redaction_configs":[],"raw_object_store":null}"#;

/// The complete legacy payload, manifest included, exactly as an archive
/// written before the personal MCP OAuth domain carried it.
pub(super) const LEGACY_PAYLOAD_JSON: &str = r#"{"manifest":{"format_version":2,"backend_kind":"postgres","exported_at":"2023-11-14T22:13:20Z","payload_fingerprint":"9014ce704f01c7019ff78fa96817d60fc16c8051bad8db0627dcfa1511fea9c6","domains":[{"name":"users","records":1,"unrecoverable_secrets":0},{"name":"client_keys","records":0,"unrecoverable_secrets":0},{"name":"endpoints","records":0,"unrecoverable_secrets":0},{"name":"model_routes","records":0,"unrecoverable_secrets":0},{"name":"mcp_servers","records":0,"unrecoverable_secrets":0},{"name":"mcp_credentials","records":0,"unrecoverable_secrets":0},{"name":"settings","records":1,"unrecoverable_secrets":0},{"name":"user_redaction_configs","records":0,"unrecoverable_secrets":0},{"name":"raw_object_store","records":0,"unrecoverable_secrets":0}]},"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:20Z","updated_at":"2023-11-14T22:13:20Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[],"settings":[{"key":"redaction_enabled","version":1,"value":true,"updated_at":null}],"user_redaction_configs":[],"raw_object_store":null}"#;

/// The payload fingerprint the legacy archive carried.
pub(super) const LEGACY_FINGERPRINT: &str =
    "9014ce704f01c7019ff78fa96817d60fc16c8051bad8db0627dcfa1511fea9c6";

fn seeded_user(user_id: i64, login: &str) -> UserSnapshot {
    let stamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("timestamp");
    UserSnapshot {
        user_id,
        login_name: login.to_string(),
        display_name: login.to_string(),
        password_hash: Some("$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_string()),
        is_admin: true,
        is_active: true,
        created_at: stamp,
        updated_at: stamp,
    }
}

pub(super) fn sample_snapshot(backend: ConfigBackendKind) -> ConfigSnapshot {
    let domains = SnapshotDomains {
        users: vec![seeded_user(1, "admin")],
        settings: vec![SettingSnapshot {
            key: "redaction_enabled".to_string(),
            version: 1,
            value: serde_json::Value::Bool(true),
            updated_at: None,
        }],
        ..empty_domains()
    };
    ConfigSnapshot::new(backend, domains)
}

/// The identifier of the archived personal OAuth server.
pub(super) const GRANT_SERVER_ID: &str = "3f1b5f3e-6b0a-4a52-9a1a-2d9d2f0b7c11";
/// The owner of the archived personal OAuth server.
pub(super) const GRANT_OWNER_USER_ID: i64 = 1;

/// One personal MCP OAuth grant with every portable field filled.
pub(super) fn sample_grant() -> crate::db::McpOAuthGrantSnapshot {
    crate::db::McpOAuthGrantSnapshot {
        server_id: uuid::Uuid::parse_str(GRANT_SERVER_ID).expect("grant server id"),
        owner_user_id: GRANT_OWNER_USER_ID,
        config_fingerprint: "fingerprint-of-the-archived-server".to_string(),
        registration: crate::db::McpOAuthRegistration {
            snapshot: serde_json::json!({
                "client_info": { "client_id": "archived-client", "redirect_uris": [] },
                "authorization_servers": ["https://auth.example.invalid"]
            }),
        },
        credentials: crate::db::McpOAuthStoredCredentials {
            engine: serde_json::json!({
                "access_token": "archived-access-token",
                "token_type": "Bearer",
                "client_info": { "client_id": "archived-client" }
            }),
            scopes: vec!["mcp:read".to_string(), "mcp:write".to_string()],
            expires_at: Some(
                chrono::DateTime::from_timestamp(1_800_000_000, 0).expect("grant expiry"),
            ),
            issuer: Some("https://auth.example.invalid".to_string()),
        },
    }
}

/// A snapshot that carries one personal MCP OAuth grant next to its owner.
pub(super) fn snapshot_with_grant(backend: ConfigBackendKind) -> ConfigSnapshot {
    let mut domains = empty_domains();
    domains.users = vec![seeded_user(GRANT_OWNER_USER_ID, "admin")];
    domains.mcp_oauth = vec![sample_grant()];
    ConfigSnapshot::new(backend, domains)
}

/// The domain records a payload carried while the MCP credential money fields
/// were still exported, including non-default financial values. Frozen bytes:
/// they must keep decoding and fingerprinting exactly as they did.
pub(super) const LEGACY_MONEY_DOMAINS_JSON: &str = r#"{"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:21Z","updated_at":"2023-11-14T22:13:21Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[{"credential_id":"5c2f0d9e-0a1b-4c3d-8e9f-0a1b2c3d4e5f","server_id":"3f1b5f3e-6b0a-4a52-9a1a-2d9d2f0b7c11","credential_label":"token-1","secret":"fc-archived-secret","position":0,"enabled":true,"provider_kind":"firecrawl","default_cost":0.25,"strict_mode":true,"billing_period_start":"2023-11-14T22:13:21Z","billing_period_end":"2023-11-14T22:13:21Z","created_at":"2023-11-14T22:13:21Z","updated_at":"2023-11-14T22:13:21Z"}],"settings":[],"user_redaction_configs":[],"raw_object_store":null}"#;

/// The complete payload for [`LEGACY_MONEY_DOMAINS_JSON`], manifest included.
pub(super) const LEGACY_MONEY_PAYLOAD_JSON: &str = r#"{"manifest":{"format_version":2,"backend_kind":"postgres","exported_at":"2023-11-14T22:13:21Z","payload_fingerprint":"7339c33fcc6f37b118e8d5954f5999aeb2103b5b8e65b4d3c93901ac52eceaa5","domains":[{"name":"users","records":1,"unrecoverable_secrets":0},{"name":"client_keys","records":0,"unrecoverable_secrets":0},{"name":"endpoints","records":0,"unrecoverable_secrets":0},{"name":"model_routes","records":0,"unrecoverable_secrets":0},{"name":"mcp_servers","records":0,"unrecoverable_secrets":0},{"name":"mcp_credentials","records":1,"unrecoverable_secrets":0},{"name":"settings","records":0,"unrecoverable_secrets":0},{"name":"user_redaction_configs","records":0,"unrecoverable_secrets":0},{"name":"raw_object_store","records":0,"unrecoverable_secrets":0},{"name":"mcp_oauth","records":0,"unrecoverable_secrets":0}]},"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:21Z","updated_at":"2023-11-14T22:13:21Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[{"credential_id":"5c2f0d9e-0a1b-4c3d-8e9f-0a1b2c3d4e5f","server_id":"3f1b5f3e-6b0a-4a52-9a1a-2d9d2f0b7c11","credential_label":"token-1","secret":"fc-archived-secret","position":0,"enabled":true,"provider_kind":"firecrawl","default_cost":0.25,"strict_mode":true,"billing_period_start":"2023-11-14T22:13:21Z","billing_period_end":"2023-11-14T22:13:21Z","created_at":"2023-11-14T22:13:21Z","updated_at":"2023-11-14T22:13:21Z"}],"settings":[],"user_redaction_configs":[],"raw_object_store":null}"#;

/// The payload fingerprint [`LEGACY_MONEY_PAYLOAD_JSON`] carried.
pub(super) const LEGACY_MONEY_FINGERPRINT: &str =
    "7339c33fcc6f37b118e8d5954f5999aeb2103b5b8e65b4d3c93901ac52eceaa5";

/// Frozen native format-3 payload written before explicit per-user endpoint
/// preferences became portable. It declares the nine old domains, carries no
/// preference field, and its fingerprint is over these exact domain bytes.
pub(super) const V3_NINE_DOMAINS_JSON: &str = r#"{"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:20Z","updated_at":"2023-11-14T22:13:20Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[],"settings":[{"key":"redaction_enabled","version":1,"value":true,"updated_at":null}],"user_redaction_configs":[]}"#;

pub(super) const V3_NINE_FINGERPRINT: &str =
    "1e3b3b347a9a177bbb3ecbe67eb09fa0ed715dddf172ed194b317accaf814bca";

pub(super) const V3_NINE_PAYLOAD_JSON: &str = r#"{"manifest":{"format_version":3,"backend_kind":"postgres","exported_at":"2023-11-14T22:13:20Z","payload_fingerprint":"1e3b3b347a9a177bbb3ecbe67eb09fa0ed715dddf172ed194b317accaf814bca","domains":[{"name":"users","records":0,"unrecoverable_secrets":0},{"name":"client_keys","records":0,"unrecoverable_secrets":0},{"name":"endpoints","records":0,"unrecoverable_secrets":0},{"name":"model_routes","records":0,"unrecoverable_secrets":0},{"name":"mcp_servers","records":0,"unrecoverable_secrets":0},{"name":"mcp_credentials","records":0,"unrecoverable_secrets":0},{"name":"settings","records":0,"unrecoverable_secrets":0},{"name":"user_redaction_configs","records":0,"unrecoverable_secrets":0},{"name":"mcp_oauth","records":0,"unrecoverable_secrets":0}]},"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:20Z","updated_at":"2023-11-14T22:13:20Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[],"settings":[{"key":"redaction_enabled","version":1,"value":true,"updated_at":null}],"user_redaction_configs":[]}"#;
