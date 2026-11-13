use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use sqlx::Row;
use uuid::Uuid;

use crate::db::entities::sqlite::standalone_endpoint_keys::{
    ActiveModel as EndpointKeyActiveModel, Column as EndpointKeyColumn, Entity as EndpointKeyEntity,
};
use crate::db::entities::sqlite::standalone_provider_endpoints::{
    ActiveModel as ProviderEndpointActiveModel, Column as ProviderEndpointColumn,
    Entity as ProviderEndpointEntity,
};
use crate::db::seaorm::sqlite_connection;
use crate::db::{EndpointOAuthToken, EndpointOAuthTokenSet};
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::models::EndpointOAuthTokenConfig;
use crate::standalone_config::rows;
use crate::standalone_config::write::{
    self, EncryptedConfig, EncryptedEndpoint, EnvelopePart, envelope_part, envelope_version,
};
use crate::standalone_config::{
    EndpointApiKeyConfig, ProviderEndpointConfig, Result, StandaloneConfig, StandaloneConfigError,
};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    // ---- Row-level reads for the unified configuration repository. ----

    pub async fn list_endpoints_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ProviderEndpointConfig>)> {
        let db = sqlite_connection(&self.pool);
        let total = ProviderEndpointEntity::find()
            .count(&db)
            .await
            .map_err(rows::native_error)? as i64;
        let models = ProviderEndpointEntity::find()
            .order_by_asc(ProviderEndpointColumn::Name)
            .limit(rows.clamp(1, 200) as u64)
            .offset(first.max(0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let mut endpoints = Vec::with_capacity(models.len());
        for model in models {
            let (endpoint, envelope, proxy_envelope) = rows::endpoint_from_model(&model)?;
            endpoints.push(decode_endpoint(
                manager,
                endpoint,
                envelope,
                proxy_envelope,
            )?);
        }
        Ok((total, endpoints))
    }

    pub async fn get_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<ProviderEndpointConfig>> {
        let db = sqlite_connection(&self.pool);
        let model = ProviderEndpointEntity::find_by_id(endpoint_id.to_string())
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        let Some(model) = model else { return Ok(None) };
        let (mut endpoint, envelope, proxy_envelope) = rows::endpoint_from_model(&model)?;
        endpoint = decode_endpoint(manager, endpoint, envelope, proxy_envelope)?;
        endpoint.api_keys = self.list_endpoint_keys_for(manager, endpoint_id).await?;
        Ok(Some(endpoint))
    }

    // ---- Issue #589: optional per-endpoint OpenAI Admin API key (envelope storage). ----

    /// Store (or clear, with `None`/empty) the optional OpenAI Admin API key
    /// for an endpoint. The secret is encrypted into the 0035 envelope
    /// columns; admin responses only ever expose the saved indicator.
    pub async fn set_endpoint_admin_api_key(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
        value: Option<&str>,
    ) -> Result<()> {
        let normalized = value.map(str::trim).filter(|value| !value.is_empty());
        let envelope = normalized
            .map(|value| manager.encrypt(value))
            .transpose()
            .map_err(StandaloneConfigError::Encryption)?;
        let now = rfc3339_now();
        standalone_query!("src/sql/standalone/set_endpoint_admin_key.sql")
            .bind(endpoint_id.to_string())
            .bind(envelope_part(&envelope, EnvelopePart::Ciphertext))
            .bind(envelope_part(&envelope, EnvelopePart::Nonce))
            .bind(envelope_version(&envelope))
            .bind(now.clone())
            .bind(now)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Decrypted Admin API key for an endpoint, used by the server-side PATCH
    /// carry and by the organization usage read. `None` means not configured;
    /// the value is never echoed by an admin response.
    pub async fn endpoint_admin_api_key(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<String>> {
        let row = standalone_query!("src/sql/standalone/get_endpoint_admin_key.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(self.pool())
            .await?;
        let Some(row) = row else { return Ok(None) };
        let envelope = rows::envelope(&row, "admin_api_key")?;
        write::decrypt_optional(manager, envelope.as_ref())
    }

    /// Endpoint ids that currently store an Admin API key, so a page listing
    /// resolves the saved indicator without a query per row.
    pub async fn list_endpoint_admin_key_ids(&self) -> Result<Vec<Uuid>> {
        let rows = standalone_query!("src/sql/standalone/list_endpoint_admin_key_ids.sql")
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|row| rows::uuid(row, "endpoint_id"))
            .collect()
    }

    pub async fn list_endpoint_keys_for(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Vec<EndpointApiKeyConfig>> {
        let db = sqlite_connection(&self.pool);
        let models = EndpointKeyEntity::find()
            .filter(EndpointKeyColumn::EndpointId.eq(endpoint_id.to_string()))
            .order_by_asc(EndpointKeyColumn::Position)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        models
            .into_iter()
            .map(|model| {
                let (mut key, envelope) = rows::endpoint_key_from_model(&model)?;
                key.api_key = manager.decrypt(&envelope)?;
                Ok(key)
            })
            .collect()
    }

    // ---- Issue #599 R2a: per-endpoint ChatGPT OAuth token (envelope storage). ----

    /// Store (or refresh) the ChatGPT OAuth token for an endpoint, or clear
    /// it with `None` (NULL convention: all token columns go NULL so presence
    /// reads fall back to absent). Both secrets are encrypted through the
    /// manager into the 0033 envelope triplets; presence afterwards reads as
    /// `refresh_token IS NOT NULL`.
    pub async fn set_endpoint_oauth_token(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: Uuid,
        token: Option<EndpointOAuthTokenSet>,
    ) -> Result<()> {
        let record = token.map(|token| EndpointOAuthTokenConfig {
            endpoint_id,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_at,
        });
        let access = record
            .as_ref()
            .map(|record| manager.encrypt(&record.access_token))
            .transpose()?;
        let refresh = record
            .as_ref()
            .map(|record| manager.encrypt(&record.refresh_token))
            .transpose()?;
        let now = rfc3339_now();
        standalone_query!("src/sql/standalone/set_endpoint_oauth_token.sql")
            .bind(endpoint_id.to_string())
            .bind(envelope_part(&access, EnvelopePart::Ciphertext))
            .bind(envelope_part(&access, EnvelopePart::Nonce))
            .bind(envelope_version(&access))
            .bind(envelope_part(&refresh, EnvelopePart::Ciphertext))
            .bind(envelope_part(&refresh, EnvelopePart::Nonce))
            .bind(envelope_version(&refresh))
            .bind(record.as_ref().and_then(|record| {
                record
                    .expires_at
                    .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
            }))
            .bind(now.clone())
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Load the decrypted OAuth token. Returns `None` when no row exists or
    /// the row was cleared (NULL refresh envelope); a half-written row
    /// (refresh present, access missing) is corrupt and fails closed.
    pub async fn get_endpoint_oauth_token(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: Uuid,
    ) -> Result<Option<EndpointOAuthToken>> {
        let row = standalone_query!("src/sql/standalone/get_endpoint_oauth_token.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let access = rows::envelope(&row, "access_token")?;
        let refresh = rows::envelope(&row, "refresh_token")?;
        let Some(refresh) = refresh else {
            return Ok(None);
        };
        let Some(access) = access else {
            return Err(StandaloneConfigError::CorruptDatabase(
                "endpoint oauth token is missing its access token".to_string(),
            ));
        };
        let expires_at = row
            .try_get::<Option<String>, _>("expires_at")?
            .map(|raw| {
                chrono::DateTime::parse_from_rfc3339(&raw)
                    .map(|value| value.with_timezone(&chrono::Utc))
                    .map_err(|_| {
                        StandaloneConfigError::CorruptDatabase(
                            "endpoint oauth token has an invalid expires_at".to_string(),
                        )
                    })
            })
            .transpose()?;
        Ok(Some(EndpointOAuthToken {
            endpoint_id,
            access_token: manager.decrypt(&access)?,
            refresh_token: manager.decrypt(&refresh)?,
            expires_at,
        }))
    }

    /// IDs of endpoints with a stored (non-cleared) OAuth token. Drives the
    /// derived plan and the admin `has_oauth_token` indicator without ever
    /// touching the secrets themselves.
    pub async fn list_endpoint_oauth_token_ids(&self) -> Result<Vec<Uuid>> {
        let rows = standalone_query!("src/sql/standalone/list_endpoint_oauth_token_ids.sql")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|row| rows::uuid(row, "endpoint_id"))
            .collect()
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
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        delete_endpoint_keys(&transaction, endpoint.endpoint_id).await?;
        upsert_endpoint(&transaction, &encrypted.endpoints[0]).await?;
        for (key, envelope) in &encrypted.endpoints[0].api_keys {
            upsert_endpoint_key(&transaction, key, envelope).await?;
        }
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn set_endpoint_mcp_enabled(
        &self,
        endpoint_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<()> {
        let db = sqlite_connection(&self.pool);
        ProviderEndpointEntity::update_many()
            .set(ProviderEndpointActiveModel {
                mcp_enabled: Set(i32::from(enabled)),
                updated_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            })
            .filter(ProviderEndpointColumn::EndpointId.eq(endpoint_id.to_string()))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn delete_endpoint(&self, endpoint_id: uuid::Uuid) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        delete_endpoint_keys(&transaction, endpoint_id).await?;
        let removed = ProviderEndpointEntity::delete_many()
            .filter(ProviderEndpointColumn::EndpointId.eq(endpoint_id.to_string()))
            .exec(&transaction)
            .await
            .map_err(rows::native_error)?
            .rows_affected
            > 0;
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(removed)
    }
}

fn decode_endpoint(
    manager: &RelaySecretManager,
    mut endpoint: ProviderEndpointConfig,
    envelope: Option<crate::relay_secrets::EncryptedSecretEnvelope>,
    proxy_envelope: Option<crate::relay_secrets::EncryptedSecretEnvelope>,
) -> Result<ProviderEndpointConfig> {
    endpoint.api_key = write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
        StandaloneConfigError::CorruptDatabase("endpoint is missing its API key".to_string())
    })?;
    endpoint.proxy_url = write::decrypt_optional(manager, proxy_envelope.as_ref())?
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    Ok(endpoint)
}

async fn delete_endpoint_keys<C: ConnectionTrait>(
    connection: &C,
    endpoint_id: uuid::Uuid,
) -> Result<()> {
    EndpointKeyEntity::delete_many()
        .filter(EndpointKeyColumn::EndpointId.eq(endpoint_id.to_string()))
        .exec(connection)
        .await
        .map_err(rows::native_error)?;
    Ok(())
}

async fn upsert_endpoint<C: ConnectionTrait>(
    connection: &C,
    endpoint: &EncryptedEndpoint,
) -> Result<()> {
    ProviderEndpointEntity::insert(ProviderEndpointActiveModel {
        endpoint_id: Set(endpoint.endpoint.endpoint_id.to_string()),
        name: Set(endpoint.endpoint.name.clone()),
        provider: Set(endpoint.endpoint.provider.as_str().to_string()),
        provider_region: Set(endpoint
            .endpoint
            .provider_region
            .map(|value| value.as_str().to_string())),
        service_tier: Set(endpoint.endpoint.service_tier.clone()),
        base_url: Set(endpoint.endpoint.base_url.clone()),
        native_api: Set(endpoint.endpoint.native_api.as_str().to_string()),
        native_api_source: Set(endpoint.endpoint.native_api_source.as_str().to_string()),
        key_lb_enabled: Set(i32::from(endpoint.endpoint.key_lb_enabled)),
        enabled: Set(i32::from(endpoint.endpoint.enabled)),
        mcp_enabled: Set(i32::from(endpoint.endpoint.mcp_enabled)),
        api_key_ciphertext: Set(endpoint.api_key.ciphertext.clone()),
        api_key_nonce: Set(endpoint.api_key.nonce.clone()),
        api_key_key_version: Set(i32::from(endpoint.api_key.key_version)),
        proxy_url_ciphertext: Set(endpoint
            .proxy_url
            .as_ref()
            .map(|envelope| envelope.ciphertext.clone())),
        proxy_url_nonce: Set(endpoint
            .proxy_url
            .as_ref()
            .map(|envelope| envelope.nonce.clone())),
        proxy_url_key_version: Set(endpoint
            .proxy_url
            .as_ref()
            .map(|envelope| i32::from(envelope.key_version))),
        active_windows: Set(endpoint
            .endpoint
            .active_windows
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)),
        created_at: Set(timestamp(endpoint.endpoint.created_at)),
        updated_at: Set(timestamp(endpoint.endpoint.updated_at)),
    })
    .on_conflict(
        OnConflict::column(ProviderEndpointColumn::EndpointId)
            .update_columns([
                ProviderEndpointColumn::Name,
                ProviderEndpointColumn::Provider,
                ProviderEndpointColumn::ProviderRegion,
                ProviderEndpointColumn::ServiceTier,
                ProviderEndpointColumn::BaseUrl,
                ProviderEndpointColumn::NativeApi,
                ProviderEndpointColumn::NativeApiSource,
                ProviderEndpointColumn::KeyLbEnabled,
                ProviderEndpointColumn::Enabled,
                ProviderEndpointColumn::McpEnabled,
                ProviderEndpointColumn::ApiKeyCiphertext,
                ProviderEndpointColumn::ApiKeyNonce,
                ProviderEndpointColumn::ApiKeyKeyVersion,
                ProviderEndpointColumn::ProxyUrlCiphertext,
                ProviderEndpointColumn::ProxyUrlNonce,
                ProviderEndpointColumn::ProxyUrlKeyVersion,
                ProviderEndpointColumn::ActiveWindows,
                ProviderEndpointColumn::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(rows::native_error)?;
    Ok(())
}

async fn upsert_endpoint_key<C: ConnectionTrait>(
    connection: &C,
    key: &EndpointApiKeyConfig,
    envelope: &crate::relay_secrets::EncryptedSecretEnvelope,
) -> Result<()> {
    EndpointKeyEntity::insert(EndpointKeyActiveModel {
        key_id: Set(key.key_id.to_string()),
        endpoint_id: Set(key.endpoint_id.to_string()),
        key_label: Set(key.key_label.clone()),
        enabled: Set(i32::from(key.enabled)),
        position: Set(key.position),
        api_key_ciphertext: Set(envelope.ciphertext.clone()),
        api_key_nonce: Set(envelope.nonce.clone()),
        api_key_key_version: Set(i32::from(envelope.key_version)),
        created_at: Set(timestamp(key.created_at)),
        updated_at: Set(timestamp(key.updated_at)),
    })
    .on_conflict(
        OnConflict::column(EndpointKeyColumn::KeyId)
            .update_columns([
                EndpointKeyColumn::EndpointId,
                EndpointKeyColumn::KeyLabel,
                EndpointKeyColumn::Enabled,
                EndpointKeyColumn::Position,
                EndpointKeyColumn::ApiKeyCiphertext,
                EndpointKeyColumn::ApiKeyNonce,
                EndpointKeyColumn::ApiKeyKeyVersion,
                EndpointKeyColumn::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(rows::native_error)?;
    Ok(())
}

fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// Issue #599 R2a: `to_rfc3339()` timestamps for the OAuth token bookkeeping
/// columns (created/updated mirror the `set_endpoint_mcp_enabled` style).
fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339()
}
