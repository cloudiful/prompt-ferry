use sqlx::Row;

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::write::{self, EncryptedConfig};
use crate::standalone_config::{ManagedRelayConfig, Result, StandaloneConfig};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
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
        Ok(Some(rows::relay_envelopes(&row)?))
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

    pub async fn delete_relay(&self, relay_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_relay.sql")
            .bind(relay_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
