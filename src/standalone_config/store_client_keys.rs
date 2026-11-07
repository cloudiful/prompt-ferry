use sqlx::Row;

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::write::{self, EncryptedConfig};
use crate::standalone_config::{ClientKeyConfig, Result, StandaloneConfig};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
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

    pub async fn delete_client_key(&self, key_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_client_key.sql")
            .bind(key_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
