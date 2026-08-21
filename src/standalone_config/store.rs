use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool, sqlite::Sqlite};

use super::write::{self, EncryptedConfig};
use super::{
    BootstrapSeed, ClientKeyConfig, ManagedRelayConfig, ModelRouteConfig, ProviderEndpointConfig,
    Result, SettingConfig, StandaloneConfig, StandaloneConfigError, rows,
};
use crate::relay_secrets::RelaySecretManager;

const CURRENT_SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapOutcome {
    pub seeded: bool,
}

pub struct StandaloneConfigStore {
    pool: SqlitePool,
    path: PathBuf,
}

impl std::fmt::Debug for StandaloneConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StandaloneConfigStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl StandaloneConfigStore {
    /// Open or create a local SQLite configuration file and apply standalone migrations.
    ///
    /// The parent directory must already exist. A missing file is created; malformed,
    /// inaccessible, or future-version files return an explicit error. Secret-bearing
    /// operations separately require the caller to provide the configured
    /// `RelaySecretManager`; this store never creates or persists a master key.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "database path",
                message: "must not be empty".to_string(),
            });
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(StandaloneConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "SQLite parent directory does not exist: {}",
                        parent.display()
                    ),
                )));
            }
        }
        let pool = crate::db::connect_sqlite(path).await?;
        let store = Self {
            pool,
            path: path.to_path_buf(),
        };
        if let Err(error) = store.migrate().await {
            store.pool.close().await;
            return Err(error);
        }
        Ok(store)
    }

    pub async fn migrate(&self) -> Result<()> {
        crate::db::migrate_standalone(&self.pool)
            .await
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!("migration failed: {error}"))
            })?;
        let row = standalone_query!("src/sql/standalone/schema_version.sql")
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "schema metadata query failed: {error}"
                ))
            })?;
        let version = row
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase("schema version row is missing".to_string())
            })?
            .try_get::<i64, _>("schema_version")
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "schema version value is invalid: {error}"
                ))
            })?;
        if version != CURRENT_SCHEMA_VERSION {
            return Err(StandaloneConfigError::UnsupportedSchemaVersion {
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    pub async fn load_snapshot(&self, manager: &RelaySecretManager) -> Result<StandaloneConfig> {
        let mut transaction = self.pool.begin().await?;
        let snapshot = StandaloneConfig {
            relays: Self::load_relays(&mut transaction, manager).await?,
            endpoints: Self::load_endpoints(&mut transaction, manager).await?,
            routes: Self::load_routes(&mut transaction).await?,
            client_keys: Self::load_client_keys(&mut transaction, manager).await?,
            settings: Self::load_settings(&mut transaction).await?,
        };
        snapshot.validate()?;
        transaction.commit().await?;
        Ok(snapshot)
    }

    pub async fn replace_snapshot(
        &self,
        manager: &RelaySecretManager,
        snapshot: &StandaloneConfig,
    ) -> Result<()> {
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, snapshot)?;
        let mut transaction = self.pool.begin().await?;
        write::delete_all(&mut transaction).await?;
        write::insert_all(&mut transaction, &encrypted).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn bootstrap_if_empty(
        &self,
        manager: &RelaySecretManager,
        seed: BootstrapSeed,
    ) -> Result<BootstrapOutcome> {
        let snapshot = seed.into_config()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        let count = standalone_query!("src/sql/standalone/configuration_count.sql")
            .fetch_one(&mut *transaction)
            .await?
            .try_get::<i64, _>("record_count")?;
        if count != 0 {
            transaction.rollback().await?;
            return Ok(BootstrapOutcome { seeded: false });
        }
        write::insert_all(&mut transaction, &encrypted).await?;
        transaction.commit().await?;
        Ok(BootstrapOutcome { seeded: true })
    }

    pub async fn save_relay(
        &self,
        manager: &RelaySecretManager,
        relay: &ManagedRelayConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            relays: vec![relay.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        write::insert_relay(&mut transaction, &encrypted.relays[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint: &ProviderEndpointConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            endpoints: vec![endpoint.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_endpoint_keys.sql")
            .bind(endpoint.endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_endpoint(&mut transaction, &encrypted.endpoints[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_route(&self, route: &ModelRouteConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            routes: vec![route.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(route.rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_route(&mut transaction, route).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_client_key(
        &self,
        manager: &RelaySecretManager,
        client_key: &ClientKeyConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            client_keys: vec![client_key.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        write::insert_client_key(&mut transaction, &encrypted.client_keys[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_setting(&self, setting: &SettingConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            settings: vec![setting.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        standalone_query!("src/sql/standalone/save_setting.sql")
            .bind(&setting.key)
            .bind(setting.version)
            .bind(serde_json::to_string(&setting.value)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn load_relays(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ManagedRelayConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_relays.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut relay, envelopes) = rows::relay(&row)?;
                relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
                relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
                relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
                relay.bridge_encryption_key =
                    write::decrypt_optional(manager, envelopes[3].as_ref())?;
                Ok(relay)
            })
            .collect()
    }

    async fn load_endpoints(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ProviderEndpointConfig>> {
        let endpoint_rows = standalone_query!("src/sql/standalone/list_endpoints.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let key_rows = standalone_query!("src/sql/standalone/list_endpoint_keys.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let mut endpoints = Vec::with_capacity(endpoint_rows.len());
        for row in endpoint_rows {
            let (mut endpoint, envelope) = rows::endpoint(&row)?;
            endpoint.api_key =
                write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint is missing its API key".to_string(),
                    )
                })?;
            endpoints.push(endpoint);
        }
        for row in key_rows {
            let endpoint_id = rows::uuid(&row, "endpoint_id")?;
            let (mut key, envelope) = rows::endpoint_key(&row)?;
            key.api_key = manager.decrypt(&envelope)?;
            endpoints
                .iter_mut()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint key references a missing endpoint".to_string(),
                    )
                })?
                .api_keys
                .push(key);
        }
        Ok(endpoints)
    }

    async fn load_routes(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<ModelRouteConfig>> {
        let route_rows = standalone_query!("src/sql/standalone/list_routes.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let target_rows = standalone_query!("src/sql/standalone/list_route_targets.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let mut routes = route_rows
            .into_iter()
            .map(|row| rows::route(&row))
            .collect::<Result<Vec<_>>>()?;
        for row in target_rows {
            let (rule_id, target) = rows::route_target(&row)?;
            routes
                .iter_mut()
                .find(|route| route.rule_id == rule_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "route target references a missing route".to_string(),
                    )
                })?
                .targets
                .push(target);
        }
        Ok(routes)
    }

    async fn load_client_keys(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ClientKeyConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_client_keys.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut key, envelope) = rows::client_key(&row)?;
                key.secret = manager.decrypt(&envelope)?;
                Ok(key)
            })
            .collect()
    }

    async fn load_settings(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<SettingConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_settings.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter().map(|row| rows::setting(&row)).collect()
    }
}
