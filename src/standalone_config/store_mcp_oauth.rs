use sqlx::Row;

use uuid::Uuid;

use crate::db::{
    McpOAuthAuthorizationState, McpOAuthConsumedState, McpOAuthCredential,
    McpOAuthCredentialSecrets, McpOAuthGrant, McpOAuthLease, McpOAuthRefreshFence,
    McpOAuthRegistration, McpOAuthStateBinding, McpOAuthStateInput, McpOAuthStoredCredentials,
    join_scopes, parse_scopes,
};
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};
use crate::standalone_config::rows;
use crate::standalone_config::{Result, StandaloneConfigError};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    fn mcp_oauth_credential_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<McpOAuthCredential> {
        Ok(McpOAuthCredential {
            server_id: rows::uuid(row, "server_id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            connection_id: rows::uuid(row, "connection_id")?,
            generation: row.try_get("generation")?,
            config_fingerprint: rows::required_string(row, "config_fingerprint")?,
            scopes: parse_scopes(&rows::optional_string(row, "scopes")?.unwrap_or_default()),
            expires_at: optional_oauth_timestamp(row, "expires_at")?,
            issuer: rows::optional_string(row, "issuer")?,
            created_at: oauth_timestamp_value(row, "created_at")?,
            updated_at: oauth_timestamp_value(row, "updated_at")?,
        })
    }

    fn mcp_oauth_state_binding_from_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<McpOAuthStateBinding> {
        Ok(McpOAuthStateBinding {
            server_id: rows::uuid(row, "server_id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            session_binding_hash: rows::required_string(row, "session_binding_hash")?,
            connection_id: rows::uuid(row, "connection_id")?,
            config_fingerprint: rows::required_string(row, "config_fingerprint")?,
            redirect_uri: rows::required_string(row, "redirect_uri")?,
            expires_at: oauth_timestamp_value(row, "expires_at")?,
        })
    }

    /// Whether an unexpired authorization state is still pending for this
    /// server, owner and current configuration fingerprint.
    pub async fn has_mcp_oauth_state(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        config_fingerprint: &str,
    ) -> Result<bool> {
        let row = standalone_query!("src/sql/standalone/mcp_oauth_has_active_state.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .bind(config_fingerprint)
            .bind(oauth_now())
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<i64, _>("active")? != 0)
    }

    /// Load the decryptable credential bundle for one server owner.
    pub async fn load_mcp_oauth_credential(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<Option<McpOAuthCredentialSecrets>> {
        let Some(row) = standalone_query!("src/sql/standalone/mcp_oauth_load_credential.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(McpOAuthCredentialSecrets {
            credential: Self::mcp_oauth_credential_from_row(&row)?,
            registration: open_oauth(manager, &mcp_oauth_envelope(&row, "registration")?)?,
            credentials: open_oauth(manager, &mcp_oauth_envelope(&row, "credentials")?)?,
        }))
    }

    /// Load only the public credential status.
    pub async fn load_mcp_oauth_credential_status(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<Option<McpOAuthCredential>> {
        let Some(row) = standalone_query!("src/sql/standalone/mcp_oauth_load_credential.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(Self::mcp_oauth_credential_from_row(&row)?))
    }

    /// Install a fresh callback grant, replacing any previous epoch.
    pub async fn install_mcp_oauth_grant(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        config_fingerprint: &str,
        grant: &McpOAuthGrant,
    ) -> Result<McpOAuthCredential> {
        let registration = seal_oauth(manager, &grant.registration)?;
        let credentials = seal_oauth(manager, &grant.credentials)?;
        let connection_id = Uuid::new_v4();
        let generation = crate::db::MCP_OAUTH_INITIAL_GENERATION;
        let scopes = join_scopes(&grant.credentials.scopes);
        let now = oauth_now();
        let expires_at = grant.credentials.expires_at.map(oauth_stamp);
        let mut tx = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/mcp_oauth_delete_states_for_owner.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .execute(&mut *tx)
            .await?;
        standalone_query!("src/sql/standalone/mcp_oauth_delete_leases_for_owner.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .execute(&mut *tx)
            .await?;
        standalone_query!("src/sql/standalone/mcp_oauth_upsert_credential.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .bind(connection_id.to_string())
            .bind(generation)
            .bind(config_fingerprint)
            .bind(registration.ciphertext)
            .bind(registration.nonce)
            .bind(i64::from(registration.key_version))
            .bind(credentials.ciphertext)
            .bind(credentials.nonce)
            .bind(i64::from(credentials.key_version))
            .bind(scopes)
            .bind(expires_at)
            .bind(grant.credentials.issuer.clone())
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let row = standalone_query!("src/sql/standalone/mcp_oauth_load_credential.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .fetch_one(&self.pool)
            .await?;
        Self::mcp_oauth_credential_from_row(&row)
    }

    /// Save a refresh result through a live lease fence.
    pub async fn save_mcp_oauth_refresh(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        config_fingerprint: &str,
        fence: McpOAuthRefreshFence,
        credentials: &McpOAuthStoredCredentials,
    ) -> Result<Option<McpOAuthCredential>> {
        let envelope = seal_oauth(manager, credentials)?;
        let scopes = join_scopes(&credentials.scopes);
        let now = oauth_now();
        let expires_at = credentials.expires_at.map(oauth_stamp);
        let affected = standalone_query!("src/sql/standalone/mcp_oauth_save_refresh.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .bind(fence.connection_id.to_string())
            .bind(fence.generation)
            .bind(config_fingerprint)
            .bind(envelope.ciphertext)
            .bind(envelope.nonce)
            .bind(i64::from(envelope.key_version))
            .bind(scopes)
            .bind(expires_at)
            .bind(credentials.issuer.clone())
            .bind(&now)
            .bind(fence.lease_owner.to_string())
            .bind(&now)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected == 0 {
            return Ok(None);
        }
        self.load_mcp_oauth_credential_status(server_id, owner_user_id)
            .await
    }

    /// Clear the stored credential and every pending state/lease for the owner.
    pub async fn clear_mcp_oauth_credential(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/mcp_oauth_delete_states_for_owner.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .execute(&mut *tx)
            .await?;
        standalone_query!("src/sql/standalone/mcp_oauth_delete_leases_for_owner.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .execute(&mut *tx)
            .await?;
        let removed = standalone_query!("src/sql/standalone/mcp_oauth_delete_credential.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        tx.commit().await?;
        Ok(removed)
    }

    /// Store one single-use authorization state.
    pub async fn create_mcp_oauth_state(
        &self,
        manager: &RelaySecretManager,
        input: &McpOAuthStateInput,
    ) -> Result<()> {
        let authorization = seal_oauth(manager, &input.authorization)?;
        let pending_client = input
            .pending_client
            .as_ref()
            .map(|registration| seal_oauth(manager, registration))
            .transpose()?;
        let (pending_ciphertext, pending_nonce, pending_key_version) =
            oauth_pending_parts(pending_client.as_ref());
        let now = oauth_now();
        standalone_query!("src/sql/standalone/mcp_oauth_insert_state.sql")
            .bind(&input.state_key_hash)
            .bind(input.server_id.to_string())
            .bind(input.owner_user_id)
            .bind(&input.session_binding_hash)
            .bind(input.connection_id.to_string())
            .bind(&input.config_fingerprint)
            .bind(&input.redirect_uri)
            .bind(authorization.ciphertext)
            .bind(authorization.nonce)
            .bind(i64::from(authorization.key_version))
            .bind(pending_ciphertext)
            .bind(pending_nonce)
            .bind(pending_key_version)
            .bind(oauth_stamp(input.expires_at))
            .bind(&now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Read the non-secret binding of an unexpired state row.
    pub async fn peek_mcp_oauth_state(
        &self,
        state_key_hash: &str,
    ) -> Result<Option<McpOAuthStateBinding>> {
        let Some(row) = standalone_query!("src/sql/standalone/mcp_oauth_peek_state.sql")
            .bind(state_key_hash)
            .bind(oauth_now())
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(Self::mcp_oauth_state_binding_from_row(&row)?))
    }

    /// Read one unexpired pending authorization without consuming it.
    ///
    /// The same digest, server, owner, session, fingerprint and expiry
    /// predicates the atomic consume uses are applied here, so the preflight
    /// cannot inspect another attempt.
    pub async fn peek_mcp_oauth_authorization(
        &self,
        manager: &RelaySecretManager,
        state_key_hash: &str,
        server_id: Uuid,
        owner_user_id: i64,
        session_binding_hash: &str,
        config_fingerprint: &str,
    ) -> Result<Option<crate::db::mcp_oauth::PendingAuthorizationState>> {
        let Some(row) =
            standalone_query!("src/sql/standalone/mcp_oauth_peek_authorization_state.sql")
                .bind(state_key_hash)
                .bind(server_id.to_string())
                .bind(owner_user_id)
                .bind(session_binding_hash)
                .bind(config_fingerprint)
                .bind(oauth_now())
                .fetch_optional(&self.pool)
                .await?
        else {
            return Ok(None);
        };
        let authorization: McpOAuthAuthorizationState = open_oauth(
            manager,
            &EncryptedSecretEnvelope {
                ciphertext: row.try_get("authorization_ciphertext")?,
                nonce: row.try_get("authorization_nonce")?,
                key_version: i16::try_from(row.try_get::<i64, _>("authorization_key_version")?)
                    .map_err(|_| {
                        StandaloneConfigError::CorruptDatabase(
                            "MCP OAuth authorization_key_version is outside the i16 range"
                                .to_string(),
                        )
                    })?,
            },
        )?;
        let pending_client = oauth_pending_envelope(
            row.try_get("pending_client_ciphertext")?,
            row.try_get("pending_client_nonce")?,
            row.try_get::<Option<i64>, _>("pending_client_key_version")?,
        )?
        .as_ref()
        .map(|envelope| open_oauth::<McpOAuthRegistration>(manager, envelope))
        .transpose()?;
        Ok(Some(crate::db::mcp_oauth::PendingAuthorizationState {
            binding: Self::mcp_oauth_state_binding_from_row(&row)?,
            authorization,
            pending_client,
        }))
    }

    /// Atomically consume one state row when everything still matches.
    pub async fn consume_mcp_oauth_state(
        &self,
        manager: &RelaySecretManager,
        state_key_hash: &str,
        owner_user_id: i64,
        session_binding_hash: &str,
        config_fingerprint: &str,
    ) -> Result<Option<McpOAuthConsumedState>> {
        let Some(row) = standalone_query!("src/sql/standalone/mcp_oauth_consume_state.sql")
            .bind(state_key_hash)
            .bind(owner_user_id)
            .bind(session_binding_hash)
            .bind(config_fingerprint)
            .bind(oauth_now())
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let authorization: McpOAuthAuthorizationState = open_oauth(
            manager,
            &EncryptedSecretEnvelope {
                ciphertext: row.try_get("authorization_ciphertext")?,
                nonce: row.try_get("authorization_nonce")?,
                key_version: i16::try_from(row.try_get::<i64, _>("authorization_key_version")?)
                    .map_err(|_| {
                        StandaloneConfigError::CorruptDatabase(
                            "MCP OAuth authorization_key_version is outside the i16 range"
                                .to_string(),
                        )
                    })?,
            },
        )?;
        let pending_client = oauth_pending_envelope(
            row.try_get("pending_client_ciphertext")?,
            row.try_get("pending_client_nonce")?,
            row.try_get::<Option<i64>, _>("pending_client_key_version")?,
        )?
        .as_ref()
        .map(|envelope| open_oauth::<McpOAuthRegistration>(manager, envelope))
        .transpose()?;
        Ok(Some(McpOAuthConsumedState {
            binding: Self::mcp_oauth_state_binding_from_row(&row)?,
            authorization,
            pending_client,
        }))
    }

    /// Claim the short refresh lease for one epoch/generation.
    pub async fn claim_mcp_oauth_lease(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        connection_id: Uuid,
        generation: i64,
        lease_owner: Uuid,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<McpOAuthLease>> {
        let expires_at = oauth_stamp(expires_at);
        let now = oauth_now();
        let affected = standalone_query!("src/sql/standalone/mcp_oauth_claim_lease.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .bind(connection_id.to_string())
            .bind(generation)
            .bind(lease_owner.to_string())
            .bind(&expires_at)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected == 0 {
            return Ok(None);
        }
        let Some(row) = standalone_query!("src/sql/standalone/mcp_oauth_load_lease.sql")
            .bind(server_id.to_string())
            .bind(owner_user_id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(McpOAuthLease {
            server_id: rows::uuid(&row, "server_id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            connection_id: rows::uuid(&row, "connection_id")?,
            generation: row.try_get("generation")?,
            lease_owner: rows::uuid(&row, "lease_owner")?,
            expires_at: oauth_timestamp_value(&row, "expires_at")?,
        }))
    }

    /// Release a lease only when it still belongs to this epoch, generation and owner.
    pub async fn release_mcp_oauth_lease(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        connection_id: Uuid,
        generation: i64,
        lease_owner: Uuid,
    ) -> Result<bool> {
        Ok(
            standalone_query!("src/sql/standalone/mcp_oauth_release_lease.sql")
                .bind(server_id.to_string())
                .bind(owner_user_id)
                .bind(connection_id.to_string())
                .bind(generation)
                .bind(lease_owner.to_string())
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
}

fn oauth_now() -> String {
    oauth_stamp(chrono::Utc::now())
}

fn oauth_stamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn oauth_timestamp_value(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<chrono::DateTime<chrono::Utc>> {
    let value: String = row.try_get(column)?;
    chrono::DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&chrono::Utc))
        .map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not an RFC3339 timestamp: {error}"
            ))
        })
}

fn optional_oauth_timestamp(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    match row.try_get::<Option<String>, _>(column)? {
        Some(_) => oauth_timestamp_value(row, column).map(Some),
        None => Ok(None),
    }
}

fn mcp_oauth_envelope(
    row: &sqlx::sqlite::SqliteRow,
    prefix: &str,
) -> Result<EncryptedSecretEnvelope> {
    rows::envelope(row, prefix)?.ok_or_else(|| {
        StandaloneConfigError::CorruptDatabase(format!("MCP OAuth envelope {prefix} is missing"))
    })
}

fn seal_oauth<T: serde::Serialize>(
    manager: &RelaySecretManager,
    value: &T,
) -> Result<EncryptedSecretEnvelope> {
    let plaintext = serde_json::to_string(value).map_err(StandaloneConfigError::Serialization)?;
    Ok(manager.encrypt(&plaintext)?)
}

fn open_oauth<T: serde::de::DeserializeOwned>(
    manager: &RelaySecretManager,
    envelope: &EncryptedSecretEnvelope,
) -> Result<T> {
    let plaintext = manager.decrypt(envelope)?;
    serde_json::from_str(&plaintext).map_err(StandaloneConfigError::Serialization)
}

fn oauth_pending_parts(
    envelope: Option<&EncryptedSecretEnvelope>,
) -> (Option<Vec<u8>>, Option<Vec<u8>>, Option<i64>) {
    match envelope {
        Some(envelope) => (
            Some(envelope.ciphertext.clone()),
            Some(envelope.nonce.clone()),
            Some(i64::from(envelope.key_version)),
        ),
        None => (None, None, None),
    }
}

fn oauth_pending_envelope(
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    key_version: Option<i64>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    match (ciphertext, nonce, key_version) {
        (None, None, None) => Ok(None),
        (Some(ciphertext), Some(nonce), Some(key_version)) => Ok(Some(EncryptedSecretEnvelope {
            ciphertext,
            nonce,
            key_version: i16::try_from(key_version).map_err(|_| {
                StandaloneConfigError::CorruptDatabase(
                    "MCP OAuth pending client key version is outside the i16 range".to_string(),
                )
            })?,
        })),
        _ => Err(StandaloneConfigError::CorruptDatabase(
            "MCP OAuth pending client envelope is incomplete".to_string(),
        )),
    }
}
