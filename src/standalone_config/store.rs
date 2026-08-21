use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool, sqlite::Sqlite};

use super::write::{self, EncryptedConfig};
use super::{
    BootstrapSeed, ClientKeyConfig, EndpointApiKeyConfig, ManagedRelayConfig, ModelRouteConfig,
    ModelRouteTargetConfig, ProviderEndpointConfig, Result, SettingConfig, StandaloneConfig,
    StandaloneConfigError, rows,
};
use crate::relay_secrets::RelaySecretManager;

const CURRENT_SCHEMA_VERSION: i64 = 4;

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

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    // ---- Row-level reads for the unified configuration repository. ----

    pub async fn list_endpoints_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ProviderEndpointConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_endpoints.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let endpoint_rows = standalone_query!("src/sql/standalone/list_endpoints_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
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
        Ok((total, endpoints))
    }

    pub async fn get_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<ProviderEndpointConfig>> {
        let row = standalone_query!("src/sql/standalone/get_endpoint.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut endpoint, envelope) = rows::endpoint(&row)?;
        endpoint.api_key =
            write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase(
                    "endpoint is missing its API key".to_string(),
                )
            })?;
        let key_rows = standalone_query!("src/sql/standalone/list_endpoint_keys_for.sql")
            .bind(endpoint_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        for row in key_rows {
            let (mut key, envelope) = rows::endpoint_key(&row)?;
            key.api_key = manager.decrypt(&envelope)?;
            endpoint.api_keys.push(key);
        }
        Ok(Some(endpoint))
    }

    pub async fn list_endpoint_keys_for(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Vec<EndpointApiKeyConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_endpoint_keys_for.sql")
            .bind(endpoint_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut key, envelope) = rows::endpoint_key(&row)?;
                key.api_key = manager.decrypt(&envelope)?;
                Ok(key)
            })
            .collect()
    }

    pub async fn list_routes_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ModelRouteConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_model_routes.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let route_rows = standalone_query!("src/sql/standalone/list_routes_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut routes = Vec::with_capacity(route_rows.len());
        for row in route_rows {
            routes.push(rows::route(&row)?);
        }
        for row in standalone_query!("src/sql/standalone/list_route_targets.sql")
            .fetch_all(&self.pool)
            .await?
        {
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
        Ok((total, routes))
    }

    pub async fn get_route(&self, rule_id: uuid::Uuid) -> Result<Option<ModelRouteConfig>> {
        let row = standalone_query!("src/sql/standalone/get_model_route.sql")
            .bind(rule_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let mut route = rows::route(&row)?;
        for row in standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?
        {
            let (_, target) = rows::route_target(&row)?;
            route.targets.push(target);
        }
        Ok(Some(route))
    }

    pub async fn list_route_targets_for(
        &self,
        rule_id: uuid::Uuid,
    ) -> Result<Vec<ModelRouteTargetConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| Ok(rows::route_target(&row)?.1))
            .collect()
    }

    pub async fn list_relays_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<ManagedRelayConfig>)> {
        let counts = standalone_query!("src/sql/standalone/count_relays.sql")
            .fetch_one(&self.pool)
            .await?;
        let total = counts.try_get::<i64, _>("total")?;
        let enabled_count = counts
            .try_get::<Option<i64>, _>("enabled_count")?
            .unwrap_or(0);
        let relay_rows = standalone_query!("src/sql/standalone/list_relays_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut relays = Vec::with_capacity(relay_rows.len());
        for row in relay_rows {
            let (mut relay, envelopes) = rows::relay(&row)?;
            relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
            relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
            relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
            relay.bridge_encryption_key = write::decrypt_optional(manager, envelopes[3].as_ref())?;
            relays.push(relay);
        }
        Ok((total, enabled_count, relays))
    }

    pub async fn get_relay(
        &self,
        manager: &RelaySecretManager,
        relay_id: uuid::Uuid,
    ) -> Result<Option<ManagedRelayConfig>> {
        let row = standalone_query!("src/sql/standalone/get_relay.sql")
            .bind(relay_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut relay, envelopes) = rows::relay(&row)?;
        relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
        relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
        relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
        relay.bridge_encryption_key = write::decrypt_optional(manager, envelopes[3].as_ref())?;
        Ok(Some(relay))
    }

    /// Look up a managed relay and return the encrypted envelopes for its
    /// TLS/bridge secrets. Used by update handlers to carry forward
    /// unchanged secrets via the `Keep` patch mode.
    pub async fn get_relay_envelopes(
        &self,
        relay_id: uuid::Uuid,
    ) -> Result<Option<[Option<crate::relay_secrets::EncryptedSecretEnvelope>; 4]>> {
        let row = standalone_query!("src/sql/standalone/get_relay_envelopes.sql")
            .bind(relay_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(super::rows::relay_envelopes(&row)?))
    }

    pub async fn list_client_keys_for(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ClientKeyConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_client_keys_for.sql")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_client_keys_for.sql")
            .bind(user_id)
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            let (mut key, envelope) = rows::client_key(&row)?;
            key.secret = manager.decrypt(&envelope)?;
            keys.push(key);
        }
        Ok((total, keys))
    }

    pub async fn get_client_key(
        &self,
        manager: &RelaySecretManager,
        key_id: uuid::Uuid,
    ) -> Result<Option<ClientKeyConfig>> {
        let row = standalone_query!("src/sql/standalone/get_client_key.sql")
            .bind(key_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut key, envelope) = rows::client_key(&row)?;
        key.secret = manager.decrypt(&envelope)?;
        Ok(Some(key))
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<SettingConfig>> {
        let row = standalone_query!("src/sql/standalone/get_setting.sql")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(rows::setting(&row)?))
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

    /// Insert or replace a single model route and its targets without running
    /// the full snapshot validator. Used by the unified configuration
    /// repository when an endpoint has just been created in the same
    /// transaction context.
    pub async fn save_route_direct(&self, route: &ModelRouteConfig) -> Result<()> {
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

    // ---- Row-level mutations for the unified configuration repository. ----

    pub async fn delete_endpoint(&self, endpoint_id: uuid::Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_endpoint_keys.sql")
            .bind(endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let result = standalone_query!("src/sql/standalone/delete_endpoint.sql")
            .bind(endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let removed = result.rows_affected() > 0;
        transaction.commit().await?;
        Ok(removed)
    }

    pub async fn delete_route(&self, rule_id: uuid::Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let result = standalone_query!("src/sql/standalone/delete_model_route.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let removed = result.rows_affected() > 0;
        transaction.commit().await?;
        Ok(removed)
    }

    pub async fn delete_relay(&self, relay_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_relay.sql")
            .bind(relay_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_client_key(&self, key_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_client_key.sql")
            .bind(key_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_setting.sql")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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
