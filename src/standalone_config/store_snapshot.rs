use sqlx::Row;
use sqlx::sqlite::Sqlite;

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::write::{self, EncryptedConfig};
use crate::standalone_config::{
    BootstrapSeed, ClientKeyConfig, ManagedRelayConfig, ModelRouteConfig, ProviderEndpointConfig,
    Result, SettingConfig, StandaloneConfig, StandaloneConfigError,
};

use super::{BootstrapOutcome, StandaloneConfigStore};

impl StandaloneConfigStore {
    pub async fn load_snapshot(&self, manager: &RelaySecretManager) -> Result<StandaloneConfig> {
        let mut transaction = self.pool.begin().await?;
        let snapshot = StandaloneConfig {
            relays: Self::load_relays(&mut transaction, manager).await?,
            endpoints: Self::load_endpoints(&mut transaction, manager).await?,
            routes: Self::load_routes(&mut transaction, manager).await?,
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
        standalone_query!("src/sql/standalone/create_snapshot_endpoint_ids.sql")
            .execute(&mut *transaction)
            .await?;
        standalone_query!("src/sql/standalone/clear_snapshot_endpoint_ids.sql")
            .execute(&mut *transaction)
            .await?;
        for endpoint in &snapshot.endpoints {
            standalone_query!("src/sql/standalone/save_snapshot_endpoint_id.sql")
                .bind(endpoint.endpoint_id.to_string())
                .execute(&mut *transaction)
                .await?;
        }
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
            let (mut endpoint, envelope, proxy_envelope) = rows::endpoint(&row)?;
            endpoint.api_key =
                write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint is missing its API key".to_string(),
                    )
                })?;
            endpoint.proxy_url = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
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
        manager: &RelaySecretManager,
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
            let (rule_id, mut target, proxy_envelope) = rows::route_target(&row)?;
            target.proxy_url_override = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
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
