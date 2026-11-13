use sea_orm::sea_query::{Expr, ExprTrait, OnConflict};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};

use crate::db::entities::sqlite::standalone_relays::{ActiveModel, Column, Entity};
use crate::db::seaorm::sqlite_connection;
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};
use crate::standalone_config::rows;
use crate::standalone_config::write;
use crate::standalone_config::{
    ManagedRelayConfig, Result, StandaloneConfig, StandaloneConfigError,
};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn list_relays_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<ManagedRelayConfig>)> {
        let db = sqlite_connection(&self.pool);
        let counts = sea_orm::sea_query::Query::select()
            .expr_as(
                Expr::col(Column::RelayId).count(),
                sea_orm::sea_query::Alias::new("total"),
            )
            .expr_as(
                Expr::cust("SUM(CASE WHEN enabled = 1 THEN 1 ELSE 0 END)"),
                sea_orm::sea_query::Alias::new("enabled_count"),
            )
            .from(Entity)
            .to_owned();
        let count_row = db
            .query_one(&counts)
            .await
            .map_err(rows::native_error)?
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase(
                    "standalone relay counts returned no row".to_string(),
                )
            })?;
        let total: i64 = count_row.try_get("", "total").map_err(rows::native_error)?;
        let enabled_count = count_row
            .try_get::<Option<i64>>("", "enabled_count")
            .map_err(rows::native_error)?
            .unwrap_or(0);
        let relays = Entity::find()
            .order_by_asc(Column::Name)
            .limit(rows.clamp(1, 200) as u64)
            .offset(std::cmp::Ord::max(first, 0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let mut decoded = Vec::with_capacity(relays.len());
        for model in relays {
            let (mut relay, envelopes) = rows::relay_from_model(&model)?;
            relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
            relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
            relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
            relay.bridge_encryption_key = write::decrypt_optional(manager, envelopes[3].as_ref())?;
            decoded.push(relay);
        }
        Ok((total, enabled_count, decoded))
    }

    pub async fn get_relay(
        &self,
        manager: &RelaySecretManager,
        relay_id: uuid::Uuid,
    ) -> Result<Option<ManagedRelayConfig>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find_by_id(relay_id.to_string())
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        let Some(model) = model else {
            return Ok(None);
        };
        let (mut relay, envelopes) = rows::relay_from_model(&model)?;
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
    ) -> Result<Option<[Option<EncryptedSecretEnvelope>; 4]>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find_by_id(relay_id.to_string())
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        let Some(model) = model else {
            return Ok(None);
        };
        Ok(Some(rows::relay_envelopes_from_model(&model)?))
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
        let relay_ca = encrypt_optional(manager, relay.relay_ca_pem.as_deref())?;
        let client_cert = encrypt_optional(manager, relay.client_cert_pem.as_deref())?;
        let client_key = encrypt_optional(manager, relay.client_key_pem.as_deref())?;
        let bridge_encryption_key =
            encrypt_optional(manager, relay.bridge_encryption_key.as_deref())?;
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        Entity::insert(ActiveModel {
            relay_id: Set(relay.relay_id.to_string()),
            name: Set(relay.name.clone()),
            relay_url: Set(relay.relay_url.clone()),
            enabled: Set(i32::from(relay.enabled)),
            tls_mode: Set(relay.tls_mode.as_str().to_string()),
            bridge_encryption_mode: Set(relay.bridge_encryption_mode.as_str().to_string()),
            relay_ca_ciphertext: Set(relay_ca
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            relay_ca_nonce: Set(relay_ca.as_ref().map(|envelope| envelope.nonce.clone())),
            relay_ca_key_version: Set(relay_ca
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            client_cert_ciphertext: Set(client_cert
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            client_cert_nonce: Set(client_cert.as_ref().map(|envelope| envelope.nonce.clone())),
            client_cert_key_version: Set(client_cert
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            client_key_ciphertext: Set(client_key
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            client_key_nonce: Set(client_key.as_ref().map(|envelope| envelope.nonce.clone())),
            client_key_key_version: Set(client_key
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            bridge_encryption_key_ciphertext: Set(bridge_encryption_key
                .as_ref()
                .map(|envelope| envelope.ciphertext.clone())),
            bridge_encryption_key_nonce: Set(bridge_encryption_key
                .as_ref()
                .map(|envelope| envelope.nonce.clone())),
            bridge_encryption_key_key_version: Set(bridge_encryption_key
                .as_ref()
                .map(|envelope| i32::from(envelope.key_version))),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(Column::RelayId)
                .update_columns([
                    Column::Name,
                    Column::RelayUrl,
                    Column::Enabled,
                    Column::TlsMode,
                    Column::BridgeEncryptionMode,
                    Column::RelayCaCiphertext,
                    Column::RelayCaNonce,
                    Column::RelayCaKeyVersion,
                    Column::ClientCertCiphertext,
                    Column::ClientCertNonce,
                    Column::ClientCertKeyVersion,
                    Column::ClientKeyCiphertext,
                    Column::ClientKeyNonce,
                    Column::ClientKeyKeyVersion,
                    Column::BridgeEncryptionKeyCiphertext,
                    Column::BridgeEncryptionKeyNonce,
                    Column::BridgeEncryptionKeyKeyVersion,
                ])
                .value(Column::UpdatedAt, Expr::current_timestamp())
                .to_owned(),
        )
        .exec(&transaction)
        .await
        .map_err(rows::native_error)?;
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn delete_relay(&self, relay_id: uuid::Uuid) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::delete_many()
            .filter(Column::RelayId.eq(relay_id.to_string()))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(result.rows_affected > 0)
    }
}

fn encrypt_optional(
    manager: &RelaySecretManager,
    value: Option<&str>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    value
        .map(|value| {
            manager
                .encrypt(value)
                .map_err(StandaloneConfigError::Encryption)
        })
        .transpose()
}
