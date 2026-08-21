//! Worker settings CRUD for the unified configuration repository.
//!
//! The repository handles every JSON-backed setting the worker reads at
//! startup (request content logging, usage retention, redaction config,
//! relay IP whitelist, model route whitelist, LLM review, etc.) and persists
//! them through the matching backend.

use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};

use super::{PostgresConfigRepository, SqliteConfigRepository};
use crate::standalone_config::SettingConfig;

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedSetting {
    pub key: String,
    pub version: i64,
    pub value: serde_json::Value,
}

impl UnifiedSetting {
    fn from_postgres(key: String, version: i64, value: serde_json::Value) -> Self {
        Self {
            key,
            version,
            value,
        }
    }

    fn from_sqlite(setting: SettingConfig) -> Self {
        Self {
            key: setting.key,
            version: setting.version,
            value: setting.value,
        }
    }
}

impl super::ConfigRepository {
    pub async fn get_json_setting<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self {
            Self::Postgres(repo) => repo.get_json_setting(key).await,
            Self::Sqlite(repo) => repo.get_json_setting(key).await,
        }
    }

    pub async fn set_json_setting<T>(&self, key: &str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        match self {
            Self::Postgres(repo) => repo.set_json_setting(key, value).await,
            Self::Sqlite(repo) => repo.set_json_setting(key, value).await,
        }
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<UnifiedSetting>> {
        match self {
            Self::Postgres(repo) => repo.get_setting(key).await,
            Self::Sqlite(repo) => repo.get_setting(key).await,
        }
    }

    pub async fn get_bool_setting(&self, key: &str, default: bool) -> Result<bool> {
        match self {
            Self::Postgres(repo) => crate::db::get_bool_setting(repo.pool(), key, default).await,
            Self::Sqlite(repo) => repo.get_bool_setting(key, default).await,
        }
    }

    pub async fn set_bool_setting(&self, key: &str, enabled: bool) -> Result<()> {
        match self {
            Self::Postgres(repo) => crate::db::set_bool_setting(repo.pool(), key, enabled).await,
            Self::Sqlite(repo) => repo.set_bool_setting(key, enabled).await,
        }
    }
}

impl PostgresConfigRepository {
    async fn get_json_setting<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        crate::db::get_json_setting(self.pool(), key).await
    }

    async fn set_json_setting<T>(&self, key: &str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        crate::db::set_json_setting(self.pool(), key, value)
            .await
            .context("failed to set JSON setting")?;
        Ok(())
    }

    async fn get_setting(&self, key: &str) -> Result<Option<UnifiedSetting>> {
        let setting = crate::db::get_json_setting::<serde_json::Value>(self.pool(), key).await?;
        let Some(value) = setting else {
            return Ok(None);
        };
        Ok(Some(UnifiedSetting::from_postgres(
            key.to_string(),
            1,
            value,
        )))
    }
}

impl SqliteConfigRepository {
    async fn get_json_setting<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(setting) = snapshot
            .settings
            .into_iter()
            .find(|setting| setting.key == key)
        else {
            return Ok(None);
        };
        let value = serde_json::from_value(setting.value)?;
        Ok(Some(value))
    }

    async fn set_json_setting<T>(&self, key: &str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let json_value = serde_json::to_value(value)?;
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut snapshot = snapshot;
        if let Some(existing) = snapshot.settings.iter_mut().find(|s| s.key == key) {
            existing.value = json_value;
            existing.version = existing.version.saturating_add(1);
        } else {
            snapshot.settings.push(SettingConfig {
                key: key.to_string(),
                version: 1,
                value: json_value,
            });
        }
        self.store
            .replace_snapshot(&self.manager, &snapshot)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }

    async fn get_setting(&self, key: &str) -> Result<Option<UnifiedSetting>> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(snapshot
            .settings
            .into_iter()
            .find(|setting| setting.key == key)
            .map(UnifiedSetting::from_sqlite))
    }

    async fn get_bool_setting(&self, key: &str, default: bool) -> Result<bool> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(setting) = snapshot
            .settings
            .into_iter()
            .find(|setting| setting.key == key)
        else {
            return Ok(default);
        };
        Ok(setting.value.as_bool().unwrap_or(default))
    }

    async fn set_bool_setting(&self, key: &str, enabled: bool) -> Result<()> {
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut snapshot = snapshot;
        let value = serde_json::Value::Bool(enabled);
        if let Some(existing) = snapshot.settings.iter_mut().find(|s| s.key == key) {
            existing.value = value;
            existing.version = existing.version.saturating_add(1);
        } else {
            snapshot.settings.push(SettingConfig {
                key: key.to_string(),
                version: 1,
                value,
            });
        }
        self.store
            .replace_snapshot(&self.manager, &snapshot)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }
}
