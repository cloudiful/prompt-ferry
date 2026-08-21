//! Managed relay CRUD for the unified configuration repository.
//!
//! The repository exposes a single backend-dispatching API for relays and
//! unified snapshot publication: SQLite writes through `StandaloneConfigStore`
//! and the encrypted secrets are persisted via the existing envelope helpers.

use anyhow::{Context, Result};
use serde::Serialize;
use uuid::Uuid;

use super::relays_map;
use super::{PostgresConfigRepository, SqliteConfigRepository};
use crate::{
    bridge::protocol::{ClientRoute, RelayIpPolicy},
    db::{ManagedRelayInput, ManagedRelayRuntimeStatus as DbManagedRelayRuntimeStatus},
    relay_secrets::EncryptedSecretEnvelope,
    worker_admin::AdminState,
};

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedManagedRelay {
    pub relay_id: Uuid,
    pub name: String,
    pub relay_url: String,
    pub enabled: bool,
    pub tls_mode: crate::config::TlsMode,
    pub bridge_encryption_mode: crate::config::BridgeEncryptionMode,
    pub has_relay_ca: bool,
    pub has_client_cert: bool,
    pub has_client_key: bool,
    pub has_bridge_key: bool,
}

impl UnifiedManagedRelay {
    pub fn attach_runtime(
        self,
        runtime: DbManagedRelayRuntimeStatus,
    ) -> crate::worker_admin_types::ManagedRelay {
        use crate::db::ManagedRelayRow;
        let now = chrono::Utc::now();
        let row = ManagedRelayRow {
            relay_id: self.relay_id,
            name: self.name,
            relay_url: self.relay_url,
            enabled: self.enabled,
            tls_mode: relays_map::tls_mode_value(self.tls_mode).to_string(),
            bridge_encryption_mode: relays_map::bridge_mode_value(self.bridge_encryption_mode)
                .to_string(),
            relay_ca_ciphertext: None,
            relay_ca_nonce: None,
            relay_ca_key_version: if self.has_relay_ca { Some(0) } else { None },
            client_cert_ciphertext: None,
            client_cert_nonce: None,
            client_cert_key_version: if self.has_client_cert { Some(0) } else { None },
            client_key_ciphertext: None,
            client_key_nonce: None,
            client_key_key_version: if self.has_client_key { Some(0) } else { None },
            bridge_encryption_key_ciphertext: None,
            bridge_encryption_key_nonce: None,
            bridge_encryption_key_key_version: if self.has_bridge_key { Some(0) } else { None },
            created_at: now,
            updated_at: now,
        };
        crate::worker_admin_types::ManagedRelay::from_parts(row, runtime)
    }
}

/// Existing sealed secrets for a managed relay, used by update handlers to
/// carry forward unchanged secrets.
#[derive(Clone)]
pub struct ManagedRelaySecrets {
    pub relay_ca: Option<EncryptedSecretEnvelope>,
    pub client_cert: Option<EncryptedSecretEnvelope>,
    pub client_key: Option<EncryptedSecretEnvelope>,
    pub bridge_key: Option<EncryptedSecretEnvelope>,
}

impl ManagedRelaySecrets {
    pub fn empty() -> Self {
        Self {
            relay_ca: None,
            client_cert: None,
            client_key: None,
            bridge_key: None,
        }
    }
}

impl super::ConfigRepository {
    pub async fn list_managed_relays_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<UnifiedManagedRelay>)> {
        match self {
            Self::Postgres(repo) => repo.list_managed_relays_page(first, rows).await,
            Self::Sqlite(repo) => repo.list_managed_relays_page(first, rows).await,
        }
    }

    pub async fn get_managed_relay(&self, relay_id: Uuid) -> Result<Option<UnifiedManagedRelay>> {
        match self {
            Self::Postgres(repo) => repo.get_managed_relay(relay_id).await,
            Self::Sqlite(repo) => repo.get_managed_relay(relay_id).await,
        }
    }

    pub async fn create_managed_relay(
        &self,
        input: ManagedRelayInput,
    ) -> Result<UnifiedManagedRelay> {
        match self {
            Self::Postgres(repo) => repo.create_managed_relay(input).await,
            Self::Sqlite(repo) => repo.create_managed_relay(input).await,
        }
    }

    pub async fn update_managed_relay(
        &self,
        relay_id: Uuid,
        input: ManagedRelayInput,
    ) -> Result<Option<UnifiedManagedRelay>> {
        match self {
            Self::Postgres(repo) => repo.update_managed_relay(relay_id, input).await,
            Self::Sqlite(repo) => repo.update_managed_relay(relay_id, input).await,
        }
    }

    pub async fn delete_managed_relay(&self, relay_id: Uuid) -> Result<bool> {
        match self {
            Self::Postgres(repo) => crate::db::delete_managed_relay(repo.pool(), relay_id).await,
            Self::Sqlite(repo) => repo.delete_managed_relay(relay_id).await,
        }
    }

    /// Look up the relay IP whitelist setting (PostgreSQL or SQLite).
    pub async fn get_relay_ip_policy(&self) -> Result<RelayIpPolicy> {
        match self {
            Self::Postgres(pg) => {
                crate::db::get_json_setting::<RelayIpPolicy>(pg.pool(), "relay_ip_whitelist")
                    .await
                    .map(|policy| policy.unwrap_or_default())
            }
            Self::Sqlite(sqlite) => {
                let snapshot = sqlite
                    .store
                    .load_snapshot(&sqlite.manager)
                    .await
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                Ok(relays_map::policy_from_settings(&snapshot.settings))
            }
        }
    }
}

impl PostgresConfigRepository {
    async fn list_managed_relays_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<UnifiedManagedRelay>)> {
        let (total, enabled_count, rows) =
            crate::db::list_managed_relays_page(&self.pool, first, rows).await?;
        Ok((
            total,
            enabled_count,
            rows.into_iter().map(relays_map::from_pg_row).collect(),
        ))
    }

    async fn get_managed_relay(&self, relay_id: Uuid) -> Result<Option<UnifiedManagedRelay>> {
        Ok(crate::db::get_managed_relay(&self.pool, relay_id)
            .await?
            .map(relays_map::from_pg_row))
    }

    async fn create_managed_relay(&self, input: ManagedRelayInput) -> Result<UnifiedManagedRelay> {
        let row = crate::db::create_managed_relay(&self.pool, input)
            .await
            .context("failed to create managed relay")?;
        Ok(relays_map::from_pg_row(row))
    }

    async fn update_managed_relay(
        &self,
        relay_id: Uuid,
        input: ManagedRelayInput,
    ) -> Result<Option<UnifiedManagedRelay>> {
        Ok(crate::db::update_managed_relay(&self.pool, relay_id, input)
            .await?
            .map(relays_map::from_pg_row))
    }
}

impl SqliteConfigRepository {
    async fn list_managed_relays_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<UnifiedManagedRelay>)> {
        let (total, enabled_count, relays) = self
            .store
            .list_relays_page(&self.manager, first, rows)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok((
            total,
            enabled_count,
            relays.into_iter().map(relays_map::from_sc).collect(),
        ))
    }

    async fn get_managed_relay(&self, relay_id: Uuid) -> Result<Option<UnifiedManagedRelay>> {
        Ok(self
            .store
            .get_relay(&self.manager, relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
            .map(relays_map::from_sc))
    }

    async fn create_managed_relay(&self, input: ManagedRelayInput) -> Result<UnifiedManagedRelay> {
        let config = relays_map::sqlite_relay_from_input(input, &self.manager)?;
        self.store
            .save_relay(&self.manager, &config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let relay = self
            .store
            .get_relay(&self.manager, config.relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
            .ok_or_else(|| anyhow::anyhow!("relay not found after insert"))?;
        Ok(relays_map::from_sc(relay))
    }

    async fn update_managed_relay(
        &self,
        relay_id: Uuid,
        input: ManagedRelayInput,
    ) -> Result<Option<UnifiedManagedRelay>> {
        let existing = self
            .store
            .get_relay(&self.manager, relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        if existing.is_none() {
            return Ok(None);
        }
        let mut config = relays_map::sqlite_relay_from_input(input, &self.manager)?;
        config.relay_id = relay_id;
        self.store
            .save_relay(&self.manager, &config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let relay = self
            .store
            .get_relay(&self.manager, relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(relay.map(relays_map::from_sc))
    }

    async fn delete_managed_relay(&self, relay_id: Uuid) -> Result<bool> {
        self.store
            .delete_relay(relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }
}

/// Snapshot payload returned by the unified snapshot publisher.
#[derive(Debug, Clone)]
pub struct UnifiedSnapshot {
    pub version: i64,
    pub keys: Vec<ClientRoute>,
    pub relay_ip_policy: RelayIpPolicy,
}

pub async fn build_unified_snapshot(
    repo: &super::ConfigRepository,
    version: i64,
) -> Result<UnifiedSnapshot> {
    let keys = build_snapshot_keys(repo).await?;
    let relay_ip_policy = repo.get_relay_ip_policy().await?;
    Ok(UnifiedSnapshot {
        version,
        keys,
        relay_ip_policy,
    })
}

pub async fn build_snapshot_keys(repo: &super::ConfigRepository) -> Result<Vec<ClientRoute>> {
    match repo {
        super::ConfigRepository::Postgres(pg) => build_snapshot_keys_postgres(pg).await,
        super::ConfigRepository::Sqlite(sqlite) => build_snapshot_keys_sqlite(sqlite).await,
    }
}

async fn build_snapshot_keys_postgres(pg: &PostgresConfigRepository) -> Result<Vec<ClientRoute>> {
    let keys = crate::db::snapshot_keys(pg.pool()).await?;
    Ok(keys
        .into_iter()
        .map(|key| ClientRoute {
            key_hash: key.key_hash,
            key_prefix: key.key_prefix,
            user_id: key.user_id,
            route_id: key.route_id.to_string(),
        })
        .collect())
}

async fn build_snapshot_keys_sqlite(sqlite: &SqliteConfigRepository) -> Result<Vec<ClientRoute>> {
    let snapshot = sqlite
        .store
        .load_snapshot(&sqlite.manager)
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    Ok(relays_map::build_snapshot_keys_sqlite(&snapshot))
}

/// Look up the existing encrypted secrets for a managed relay so an update
/// handler can treat absent/keep/clear/replace patches correctly.
pub async fn relay_secrets_for_state(
    state: &AdminState,
    relay_id: Uuid,
) -> Result<Option<ManagedRelaySecrets>> {
    if let Some(pool) = state.config_repository.as_postgres() {
        let row = crate::db::get_managed_relay(pool, relay_id).await?;
        return Ok(row.map(|r| relays_map::managed_secrets_from_row(&r)));
    }
    if let Some(repo) = state.config_repository.as_sqlite() {
        let envelopes = repo
            .store()
            .get_relay_envelopes(relay_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(envelopes) = envelopes else {
            return Ok(None);
        };
        return Ok(Some(ManagedRelaySecrets {
            relay_ca: envelopes[0].clone(),
            client_cert: envelopes[1].clone(),
            client_key: envelopes[2].clone(),
            bridge_key: envelopes[3].clone(),
        }));
    }
    Ok(None)
}
