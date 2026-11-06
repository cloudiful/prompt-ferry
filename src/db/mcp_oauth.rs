//! PostgreSQL persistence for personal MCP OAuth records (issue #1208 P2).
//!
//! Every secret is encrypted by the caller-supplied
//! [`crate::relay_secrets::RelaySecretManager`] before it reaches this module;
//! the public status types never carry an envelope.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::PgPool;
use uuid::Uuid;

use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

use crate::db::{
    McpOAuthAuthorizationState, McpOAuthConsumedState, McpOAuthCredential,
    McpOAuthCredentialSecrets, McpOAuthGrant, McpOAuthLease, McpOAuthRefreshFence,
    McpOAuthRegistration, McpOAuthStateBinding, McpOAuthStateInput, McpOAuthStoredCredentials,
    join_scopes, parse_scopes,
};

#[cfg(test)]
mod tests;

#[derive(Debug, sqlx::FromRow)]
struct CredentialRow {
    server_id: Uuid,
    owner_user_id: i64,
    connection_id: Uuid,
    generation: i64,
    config_fingerprint: String,
    registration_ciphertext: Vec<u8>,
    registration_nonce: Vec<u8>,
    registration_key_version: i16,
    credentials_ciphertext: Vec<u8>,
    credentials_nonce: Vec<u8>,
    credentials_key_version: i16,
    scopes: String,
    expires_at: Option<DateTime<Utc>>,
    issuer: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl CredentialRow {
    fn status(&self) -> McpOAuthCredential {
        McpOAuthCredential {
            server_id: self.server_id,
            owner_user_id: self.owner_user_id,
            connection_id: self.connection_id,
            generation: self.generation,
            config_fingerprint: self.config_fingerprint.clone(),
            scopes: parse_scopes(&self.scopes),
            expires_at: self.expires_at,
            issuer: self.issuer.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    fn registration(&self) -> EncryptedSecretEnvelope {
        EncryptedSecretEnvelope {
            ciphertext: self.registration_ciphertext.clone(),
            nonce: self.registration_nonce.clone(),
            key_version: self.registration_key_version,
        }
    }

    fn credentials(&self) -> EncryptedSecretEnvelope {
        EncryptedSecretEnvelope {
            ciphertext: self.credentials_ciphertext.clone(),
            nonce: self.credentials_nonce.clone(),
            key_version: self.credentials_key_version,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct StateBindingRow {
    server_id: Uuid,
    owner_user_id: i64,
    session_binding_hash: String,
    connection_id: Uuid,
    config_fingerprint: String,
    redirect_uri: String,
    expires_at: DateTime<Utc>,
}

impl StateBindingRow {
    fn binding(&self) -> McpOAuthStateBinding {
        McpOAuthStateBinding {
            server_id: self.server_id,
            owner_user_id: self.owner_user_id,
            session_binding_hash: self.session_binding_hash.clone(),
            connection_id: self.connection_id,
            config_fingerprint: self.config_fingerprint.clone(),
            redirect_uri: self.redirect_uri.clone(),
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ConsumedStateRow {
    server_id: Uuid,
    owner_user_id: i64,
    session_binding_hash: String,
    connection_id: Uuid,
    config_fingerprint: String,
    redirect_uri: String,
    expires_at: DateTime<Utc>,
    authorization_ciphertext: Vec<u8>,
    authorization_nonce: Vec<u8>,
    authorization_key_version: i16,
    pending_client_ciphertext: Option<Vec<u8>>,
    pending_client_nonce: Option<Vec<u8>>,
    pending_client_key_version: Option<i16>,
}

#[derive(Debug, sqlx::FromRow)]
struct LeaseRow {
    server_id: Uuid,
    owner_user_id: i64,
    connection_id: Uuid,
    generation: i64,
    lease_owner: Uuid,
    expires_at: DateTime<Utc>,
}

impl LeaseRow {
    fn lease(self) -> McpOAuthLease {
        McpOAuthLease {
            server_id: self.server_id,
            owner_user_id: self.owner_user_id,
            connection_id: self.connection_id,
            generation: self.generation,
            lease_owner: self.lease_owner,
            expires_at: self.expires_at,
        }
    }
}

fn seal<T: Serialize>(manager: &RelaySecretManager, value: &T) -> Result<EncryptedSecretEnvelope> {
    let plaintext = serde_json::to_string(value).context("serialize MCP OAuth secret")?;
    manager.encrypt(&plaintext)
}

fn open<T: DeserializeOwned>(
    manager: &RelaySecretManager,
    envelope: &EncryptedSecretEnvelope,
) -> Result<T> {
    let plaintext = manager.decrypt(envelope)?;
    serde_json::from_str(&plaintext).context("deserialize MCP OAuth secret")
}

fn pending_client_parts(
    envelope: Option<&EncryptedSecretEnvelope>,
) -> (Option<Vec<u8>>, Option<Vec<u8>>, Option<i16>) {
    match envelope {
        Some(envelope) => (
            Some(envelope.ciphertext.clone()),
            Some(envelope.nonce.clone()),
            Some(envelope.key_version),
        ),
        None => (None, None, None),
    }
}

fn pending_client_envelope(
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    key_version: Option<i16>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    match (ciphertext, nonce, key_version) {
        (None, None, None) => Ok(None),
        (Some(ciphertext), Some(nonce), Some(key_version)) => Ok(Some(EncryptedSecretEnvelope {
            ciphertext,
            nonce,
            key_version,
        })),
        _ => anyhow::bail!("stored MCP OAuth pending client envelope is incomplete"),
    }
}

/// Load the decryptable credential bundle for one server owner.
pub async fn load_credential(
    pool: &PgPool,
    manager: &RelaySecretManager,
    server_id: Uuid,
    owner_user_id: i64,
) -> Result<Option<McpOAuthCredentialSecrets>> {
    let Some(row) = sqlx::query_file_as!(
        CredentialRow,
        "src/sql/mcp_oauth/load_credentials.sql",
        server_id,
        owner_user_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(McpOAuthCredentialSecrets {
        credential: row.status(),
        registration: open(manager, &row.registration())?,
        credentials: open(manager, &row.credentials())?,
    }))
}

/// Load only the public credential status.
pub async fn load_credential_status(
    pool: &PgPool,
    server_id: Uuid,
    owner_user_id: i64,
) -> Result<Option<McpOAuthCredential>> {
    Ok(sqlx::query_file_as!(
        CredentialRow,
        "src/sql/mcp_oauth/load_credentials.sql",
        server_id,
        owner_user_id,
    )
    .fetch_optional(pool)
    .await?
    .map(|row| row.status()))
}

/// Install a fresh callback grant, replacing any previous epoch.
///
/// Pending states and leases belong to the previous epoch and are cleared in
/// the same short transaction.
pub async fn install_grant(
    pool: &PgPool,
    manager: &RelaySecretManager,
    server_id: Uuid,
    owner_user_id: i64,
    config_fingerprint: &str,
    grant: &McpOAuthGrant,
) -> Result<McpOAuthCredential> {
    let registration = seal(manager, &grant.registration)?;
    let credentials = seal(manager, &grant.credentials)?;
    let connection_id = Uuid::new_v4();
    let generation = crate::db::MCP_OAUTH_INITIAL_GENERATION;
    let scopes = join_scopes(&grant.credentials.scopes);

    let mut tx = pool.begin().await?;
    sqlx::query_file!(
        "src/sql/mcp_oauth/delete_states_for_owner.sql",
        server_id,
        owner_user_id,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "src/sql/mcp_oauth/delete_leases_for_owner.sql",
        server_id,
        owner_user_id,
    )
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query_file_as!(
        CredentialRow,
        "src/sql/mcp_oauth/upsert_credentials.sql",
        server_id,
        owner_user_id,
        connection_id,
        generation,
        config_fingerprint,
        registration.ciphertext,
        registration.nonce,
        registration.key_version,
        credentials.ciphertext,
        credentials.nonce,
        credentials.key_version,
        scopes,
        grant.credentials.expires_at,
        grant.credentials.issuer,
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.status())
}

/// Save a refresh result through a live lease fence.
///
/// `None` covers a stale connection epoch, stale generation, changed config
/// fingerprint, or an expired/absent lease: the caller must not treat the
/// refresh as applied.
pub async fn save_refresh(
    pool: &PgPool,
    manager: &RelaySecretManager,
    server_id: Uuid,
    owner_user_id: i64,
    config_fingerprint: &str,
    fence: McpOAuthRefreshFence,
    credentials: &McpOAuthStoredCredentials,
) -> Result<Option<McpOAuthCredential>> {
    let envelope = seal(manager, credentials)?;
    let scopes = join_scopes(&credentials.scopes);
    Ok(sqlx::query_file_as!(
        CredentialRow,
        "src/sql/mcp_oauth/save_refresh.sql",
        server_id,
        owner_user_id,
        fence.connection_id,
        fence.generation,
        config_fingerprint,
        envelope.ciphertext,
        envelope.nonce,
        envelope.key_version,
        scopes,
        credentials.expires_at,
        credentials.issuer,
        fence.lease_owner,
    )
    .fetch_optional(pool)
    .await?
    .map(|row| row.status()))
}

/// Clear the stored credential and every pending state/lease for the owner.
pub async fn clear_credential(pool: &PgPool, server_id: Uuid, owner_user_id: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query_file!(
        "src/sql/mcp_oauth/delete_states_for_owner.sql",
        server_id,
        owner_user_id,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "src/sql/mcp_oauth/delete_leases_for_owner.sql",
        server_id,
        owner_user_id,
    )
    .execute(&mut *tx)
    .await?;
    let removed = sqlx::query_file!(
        "src/sql/mcp_oauth/delete_credentials.sql",
        server_id,
        owner_user_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    tx.commit().await?;
    Ok(removed)
}

/// Store one single-use authorization state.
pub async fn create_state(
    pool: &PgPool,
    manager: &RelaySecretManager,
    input: &McpOAuthStateInput,
) -> Result<()> {
    let authorization = seal(manager, &input.authorization)?;
    let pending_client = input
        .pending_client
        .as_ref()
        .map(|registration| seal(manager, registration))
        .transpose()?;
    let (pending_ciphertext, pending_nonce, pending_key_version) =
        pending_client_parts(pending_client.as_ref());
    sqlx::query_file!(
        "src/sql/mcp_oauth/insert_state.sql",
        input.state_key_hash,
        input.server_id,
        input.owner_user_id,
        input.session_binding_hash,
        input.connection_id,
        input.config_fingerprint,
        input.redirect_uri,
        authorization.ciphertext,
        authorization.nonce,
        authorization.key_version,
        pending_ciphertext,
        pending_nonce,
        pending_key_version,
        input.expires_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether an unexpired authorization state is still pending for this server,
/// owner and current configuration fingerprint.
///
/// This is a storage fact, not process-local memory: every worker that reads
/// the same store sees the same answer, and a state that expired or was
/// consumed stops reporting.
pub async fn has_active_state(
    pool: &PgPool,
    server_id: Uuid,
    owner_user_id: i64,
    config_fingerprint: &str,
) -> Result<bool> {
    Ok(sqlx::query_file_scalar!(
        "src/sql/mcp_oauth/has_active_state.sql",
        server_id,
        owner_user_id,
        config_fingerprint,
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(false))
}

/// Read the non-secret binding of an unexpired state row.
pub async fn peek_state(
    pool: &PgPool,
    state_key_hash: &str,
) -> Result<Option<McpOAuthStateBinding>> {
    Ok(sqlx::query_file_as!(
        StateBindingRow,
        "src/sql/mcp_oauth/peek_state.sql",
        state_key_hash,
    )
    .fetch_optional(pool)
    .await?
    .map(|row| row.binding()))
}

/// Decrypted pending authorization read by the callback preflight.
///
/// It is never public API data: the service only uses it to validate the
/// callback before the atomic consume, and it is dropped immediately after.
pub struct PendingAuthorizationState {
    pub binding: McpOAuthStateBinding,
    pub authorization: McpOAuthAuthorizationState,
    pub pending_client: Option<McpOAuthRegistration>,
}

/// Read one unexpired pending authorization without consuming it.
///
/// Every predicate the atomic consume requires (exact state digest, server,
/// owner, session binding, current fingerprint and expiry) is enforced by the
/// query, so a preflight can never see another server's, another owner's or a
/// stale-configuration attempt.
pub async fn peek_authorization_state(
    pool: &PgPool,
    manager: &RelaySecretManager,
    state_key_hash: &str,
    server_id: Uuid,
    owner_user_id: i64,
    session_binding_hash: &str,
    config_fingerprint: &str,
) -> Result<Option<PendingAuthorizationState>> {
    let Some(row) = sqlx::query_file_as!(
        ConsumedStateRow,
        "src/sql/mcp_oauth/peek_authorization_state.sql",
        state_key_hash,
        server_id,
        owner_user_id,
        session_binding_hash,
        config_fingerprint,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let authorization = open(
        manager,
        &EncryptedSecretEnvelope {
            ciphertext: row.authorization_ciphertext,
            nonce: row.authorization_nonce,
            key_version: row.authorization_key_version,
        },
    )?;
    let pending_client = pending_client_envelope(
        row.pending_client_ciphertext,
        row.pending_client_nonce,
        row.pending_client_key_version,
    )?
    .as_ref()
    .map(|envelope| open::<McpOAuthRegistration>(manager, envelope))
    .transpose()?;
    Ok(Some(PendingAuthorizationState {
        binding: McpOAuthStateBinding {
            server_id: row.server_id,
            owner_user_id: row.owner_user_id,
            session_binding_hash: row.session_binding_hash,
            connection_id: row.connection_id,
            config_fingerprint: row.config_fingerprint,
            redirect_uri: row.redirect_uri,
            expires_at: row.expires_at,
        },
        authorization,
        pending_client,
    }))
}

/// Atomically consume one state row when everything still matches.
pub async fn consume_state(
    pool: &PgPool,
    manager: &RelaySecretManager,
    state_key_hash: &str,
    owner_user_id: i64,
    session_binding_hash: &str,
    config_fingerprint: &str,
) -> Result<Option<McpOAuthConsumedState>> {
    let Some(row) = sqlx::query_file_as!(
        ConsumedStateRow,
        "src/sql/mcp_oauth/consume_state.sql",
        state_key_hash,
        owner_user_id,
        session_binding_hash,
        config_fingerprint,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let authorization = open(
        manager,
        &EncryptedSecretEnvelope {
            ciphertext: row.authorization_ciphertext,
            nonce: row.authorization_nonce,
            key_version: row.authorization_key_version,
        },
    )?;
    let pending_client = pending_client_envelope(
        row.pending_client_ciphertext,
        row.pending_client_nonce,
        row.pending_client_key_version,
    )?
    .as_ref()
    .map(|envelope| open::<McpOAuthRegistration>(manager, envelope))
    .transpose()?;
    Ok(Some(McpOAuthConsumedState {
        binding: McpOAuthStateBinding {
            server_id: row.server_id,
            owner_user_id: row.owner_user_id,
            session_binding_hash: row.session_binding_hash,
            connection_id: row.connection_id,
            config_fingerprint: row.config_fingerprint,
            redirect_uri: row.redirect_uri,
            expires_at: row.expires_at,
        },
        authorization,
        pending_client,
    }))
}

/// Claim the short refresh lease for one epoch/generation.
pub async fn claim_lease(
    pool: &PgPool,
    server_id: Uuid,
    owner_user_id: i64,
    connection_id: Uuid,
    generation: i64,
    lease_owner: Uuid,
    expires_at: DateTime<Utc>,
) -> Result<Option<McpOAuthLease>> {
    Ok(sqlx::query_file_as!(
        LeaseRow,
        "src/sql/mcp_oauth/claim_lease.sql",
        server_id,
        owner_user_id,
        connection_id,
        generation,
        lease_owner,
        expires_at,
    )
    .fetch_optional(pool)
    .await?
    .map(LeaseRow::lease))
}

/// Release a lease only when it still belongs to this epoch, generation and owner.
pub async fn release_lease(
    pool: &PgPool,
    server_id: Uuid,
    owner_user_id: i64,
    connection_id: Uuid,
    generation: i64,
    lease_owner: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_file!(
        "src/sql/mcp_oauth/release_lease.sql",
        server_id,
        owner_user_id,
        connection_id,
        generation,
        lease_owner,
    )
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}
