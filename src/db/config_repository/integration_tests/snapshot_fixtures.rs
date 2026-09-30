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
