//! Backend dispatch for the portable configuration snapshot.
//!
//! Users are read from the shared [`UserStore`] so both backends emit the same
//! user records (including Argon2id password hashes, never plaintext); every
//! other domain is read by the backend-specific reader module.

use anyhow::{Context, Result};

use super::snapshot::{ConfigBackendKind, ConfigSnapshot, SnapshotDomains, UserSnapshot};
use crate::{
    db::{UserStore, config_repository::ConfigRepository},
    relay_secrets::RelaySecretManager,
};

/// Build a complete, sealed configuration snapshot for the repository's
/// backend.
pub async fn build_config_snapshot(
    repository: &ConfigRepository,
    users: &UserStore,
    manager: Option<&RelaySecretManager>,
) -> Result<ConfigSnapshot> {
    let backend_kind = if repository.is_sqlite() {
        ConfigBackendKind::Sqlite
    } else {
        ConfigBackendKind::Postgres
    };
    let user_records = snapshot_users(users).await?;
    let domains = match repository {
        ConfigRepository::Postgres(_) => {
            super::snapshot_postgres::read(repository, manager, user_records).await?
        }
        ConfigRepository::Sqlite(_) => {
            super::snapshot_sqlite::read(repository, user_records).await?
        }
    };
    let mut snapshot = ConfigSnapshot::new(backend_kind, domains);
    snapshot
        .seal()
        .context("failed to seal the configuration snapshot")?;
    Ok(snapshot)
}

/// Read every user with its password hash. The hash is fetched per login
/// because neither backend exposes a bulk hash listing; configuration export
/// is an operator action, so the extra round trips are acceptable.
pub async fn snapshot_users(users: &UserStore) -> Result<Vec<UserSnapshot>> {
    let mut snapshot = Vec::new();
    for user in users.list_users().await? {
        let password_hash = users
            .get_user_password_by_login(&user.login_name)
            .await?
            .map(|row| row.password_hash);
        snapshot.push(UserSnapshot {
            user_id: user.user_id,
            login_name: user.login_name,
            display_name: user.display_name,
            password_hash,
            is_admin: user.is_admin,
            is_active: user.is_active,
            created_at: user.created_at,
            updated_at: user.updated_at,
        });
    }
    Ok(snapshot)
}

/// Empty domain payload, used by tests and as a shape anchor.
pub fn empty_domains() -> SnapshotDomains {
    SnapshotDomains {
        users: Vec::new(),
        client_keys: Vec::new(),
        endpoints: Vec::new(),
        model_routes: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_credentials: Vec::new(),
        relays: Vec::new(),
        settings: Vec::new(),
        user_redaction_configs: Vec::new(),
        raw_object_store: None,
    }
}
