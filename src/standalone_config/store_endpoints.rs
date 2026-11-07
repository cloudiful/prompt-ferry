use sqlx::Row;
use uuid::Uuid;

use crate::db::{EndpointOAuthToken, EndpointOAuthTokenSet};
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::models::EndpointOAuthTokenConfig;
use crate::standalone_config::rows;
use crate::standalone_config::write::{
    self, EncryptedConfig, EnvelopePart, envelope_part, envelope_version,
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
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_endpoint_keys.sql")
            .bind(endpoint.endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_endpoint(&mut transaction, &encrypted.endpoints[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_endpoint_mcp_enabled(
        &self,
        endpoint_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<()> {
        standalone_query!("src/sql/standalone/set_endpoint_mcp_enabled.sql")
            .bind(i64::from(enabled))
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(endpoint_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

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
}

/// Issue #599 R2a: `to_rfc3339()` timestamps for the OAuth token bookkeeping
/// columns (created/updated mirror the `set_endpoint_mcp_enabled` style).
fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339()
}
