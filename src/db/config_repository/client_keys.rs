//! Client key CRUD for the unified configuration repository.
//!
//! The repository handles the lifetime of the secret (cleartext at creation,
//! persisted via the encrypted envelope) and presents a backend-neutral
//! `UnifiedClientKey` shape so handlers do not have to choose between
//! PostgreSQL and SQLite.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{PostgresConfigRepository, SqliteConfigRepository};
use crate::{
    db::ClientKey as PgClientKey, keys::generate_client_key,
    standalone_config::ClientKeyConfig as ScClientKey,
    worker_admin_types::CreateClientKeyResponse as PgCreateClientKeyResponse,
};

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedClientKey {
    pub key_id: Uuid,
    pub user_id: i64,
    pub key_prefix: String,
    pub label: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

impl From<PgClientKey> for UnifiedClientKey {
    fn from(key: PgClientKey) -> Self {
        Self {
            key_id: Uuid::from_u64_pair(key.key_id as u64, 0),
            user_id: key.user_id,
            key_prefix: key.key_prefix,
            label: key.label,
            enabled: key.enabled,
            created_at: key.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedClientKeyCreated {
    #[serde(flatten)]
    pub key: UnifiedClientKey,
    pub secret: String,
}

impl From<PgCreateClientKeyResponse> for UnifiedClientKeyCreated {
    fn from(value: PgCreateClientKeyResponse) -> Self {
        Self {
            key: UnifiedClientKey {
                key_id: Uuid::from_u64_pair(value.key_id as u64, 0),
                user_id: value.user_id,
                key_prefix: value.key_prefix,
                label: value.label,
                enabled: value.enabled,
                created_at: value.created_at,
            },
            secret: value.secret,
        }
    }
}

impl UnifiedClientKey {
    fn from_sqlite(key: ScClientKey) -> Self {
        Self {
            key_id: key.key_id,
            user_id: key.user_id,
            key_prefix: key.key_prefix,
            label: key.label,
            enabled: key.enabled,
            created_at: Utc::now(),
        }
    }
}

impl super::ConfigRepository {
    pub async fn list_client_keys_page(
        &self,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<UnifiedClientKey>)> {
        match self {
            Self::Postgres(repo) => repo.list_client_keys_page(user_id, first, rows).await,
            Self::Sqlite(repo) => repo.list_client_keys_page(user_id, first, rows).await,
        }
    }

    pub async fn create_client_key(
        &self,
        user_id: i64,
        label: Option<&str>,
        enabled: bool,
    ) -> Result<UnifiedClientKeyCreated> {
        match self {
            Self::Postgres(repo) => repo.create_client_key(user_id, label, enabled).await,
            Self::Sqlite(repo) => repo.create_client_key(user_id, label, enabled).await,
        }
    }

    pub async fn update_client_key(
        &self,
        user_id: i64,
        key_id: Uuid,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<Option<UnifiedClientKey>> {
        match self {
            Self::Postgres(repo) => {
                repo.update_client_key(user_id, key_id, label, enabled)
                    .await
            }
            Self::Sqlite(repo) => {
                repo.update_client_key(user_id, key_id, label, enabled)
                    .await
            }
        }
    }

    pub async fn delete_client_key(&self, user_id: i64, key_id: Uuid) -> Result<bool> {
        match self {
            Self::Postgres(repo) => repo.delete_client_key(user_id, key_id).await,
            Self::Sqlite(repo) => repo.delete_client_key(user_id, key_id).await,
        }
    }

    /// Resolve a legacy i64 client-key id to the unified UUID identifier. The
    /// PostgreSQL backend stores keys under an i64 primary key so the i64 is
    /// returned as the low 64 bits of the UUID; the SQLite backend looks up
    /// the UUID by listing keys in order.
    pub async fn resolve_client_key_uuid(
        &self,
        user_id: i64,
        legacy_key_id: i64,
    ) -> Result<Option<Uuid>> {
        match self {
            Self::Postgres(_) => Ok(Some(Uuid::from_u64_pair(legacy_key_id as u64, 0))),
            Self::Sqlite(repo) => repo.resolve_client_key_uuid(user_id, legacy_key_id).await,
        }
    }
}

impl PostgresConfigRepository {
    async fn list_client_keys_page(
        &self,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<UnifiedClientKey>)> {
        let (total, keys) = crate::db::list_client_keys_page(&self.pool, user_id, first, rows)
            .await
            .context("failed to list client keys")?;
        Ok((total, keys.into_iter().map(Into::into).collect()))
    }

    async fn create_client_key(
        &self,
        user_id: i64,
        label: Option<&str>,
        _enabled: bool,
    ) -> Result<UnifiedClientKeyCreated> {
        let (secret, prefix, hash) = generate_client_key();
        let label = label.unwrap_or("Codex key");
        let key = crate::db::create_client_key(&self.pool, user_id, label, &prefix, &hash, &secret)
            .await
            .context("failed to create client key")?;
        Ok(UnifiedClientKeyCreated {
            key: UnifiedClientKey {
                key_id: Uuid::from_u64_pair(key.key_id as u64, 0),
                user_id: key.user_id,
                key_prefix: key.key_prefix,
                label: key.label,
                enabled: key.enabled,
                created_at: key.created_at,
            },
            secret,
        })
    }

    async fn update_client_key(
        &self,
        user_id: i64,
        key_id: Uuid,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<Option<UnifiedClientKey>> {
        let pg_key_id = i64::try_from(key_id.as_u128() as u64).unwrap_or(0);
        Ok(
            crate::db::update_client_key(&self.pool, user_id, pg_key_id, label, enabled)
                .await?
                .map(Into::into),
        )
    }

    async fn delete_client_key(&self, user_id: i64, key_id: Uuid) -> Result<bool> {
        let pg_key_id = i64::try_from(key_id.as_u128() as u64).unwrap_or(0);
        crate::db::delete_client_key(&self.pool, user_id, pg_key_id).await
    }
}

impl SqliteConfigRepository {
    async fn list_client_keys_page(
        &self,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<UnifiedClientKey>)> {
        let (total, keys) = self
            .store
            .list_client_keys_for(&self.manager, user_id, first, rows)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok((
            total,
            keys.into_iter()
                .map(UnifiedClientKey::from_sqlite)
                .collect(),
        ))
    }

    async fn create_client_key(
        &self,
        user_id: i64,
        label: Option<&str>,
        enabled: bool,
    ) -> Result<UnifiedClientKeyCreated> {
        let (secret, prefix, _hash) = generate_client_key();
        let key_id = Uuid::new_v4();
        let label = label.unwrap_or("Codex key").to_string();
        let config = ScClientKey {
            key_id,
            user_id,
            key_prefix: prefix.clone(),
            label: label.clone(),
            secret: secret.clone(),
            enabled,
        };
        self.store
            .save_client_key(&self.manager, &config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(UnifiedClientKeyCreated {
            key: UnifiedClientKey {
                key_id,
                user_id,
                key_prefix: prefix,
                label,
                enabled,
                created_at: Utc::now(),
            },
            secret,
        })
    }

    async fn update_client_key(
        &self,
        user_id: i64,
        key_id: Uuid,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<Option<UnifiedClientKey>> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut snapshot = snapshot;
        let mut updated = None;
        for key in snapshot.client_keys.iter_mut() {
            if key.user_id == user_id && key.key_id == key_id {
                if let Some(label) = label.clone() {
                    key.label = label.trim().to_string();
                }
                if let Some(enabled) = enabled {
                    key.enabled = enabled;
                }
                updated = Some(UnifiedClientKey {
                    key_id: key.key_id,
                    user_id: key.user_id,
                    key_prefix: key.key_prefix.clone(),
                    label: key.label.clone(),
                    enabled: key.enabled,
                    created_at: Utc::now(),
                });
                break;
            }
        }
        if updated.is_some() {
            self.store
                .replace_snapshot(&self.manager, &snapshot)
                .await
                .map_err(|err| anyhow::anyhow!("{err}"))?;
        }
        Ok(updated)
    }

    async fn delete_client_key(&self, user_id: i64, key_id: Uuid) -> Result<bool> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut snapshot = snapshot;
        let before = snapshot.client_keys.len();
        snapshot
            .client_keys
            .retain(|key| !(key.user_id == user_id && key.key_id == key_id));
        if snapshot.client_keys.len() == before {
            return Ok(false);
        }
        self.store
            .replace_snapshot(&self.manager, &snapshot)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(true)
    }

    async fn resolve_client_key_uuid(
        &self,
        user_id: i64,
        legacy_key_id: i64,
    ) -> Result<Option<Uuid>> {
        let (total, keys) = self
            .store
            .list_client_keys_for(&self.manager, user_id, 0, i64::MAX)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let index = usize::try_from(legacy_key_id).ok();
        if let Some(index) = index {
            if index < keys.len() {
                return Ok(Some(keys[index].key_id));
            }
        }
        if total == 1 && legacy_key_id == 0 {
            return Ok(keys.first().map(|k| k.key_id));
        }
        Ok(None)
    }
}
