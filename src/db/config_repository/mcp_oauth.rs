//! Personal MCP OAuth persistence dispatch for PostgreSQL and SQLite.
//!
//! This layer owns the storage-boundary checks: a secret is only read or
//! written for a user-scoped HTTP server in `oauth` auth mode whose stored
//! config fingerprint still matches the current server row. The backends
//! themselves only encrypt/decrypt and persist.

use anyhow::Result;
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::db::{
    McpOAuthConsumedState, McpOAuthCredential, McpOAuthCredentialSecrets, McpOAuthGrant,
    McpOAuthLease, McpOAuthRefreshFence, McpOAuthStateBinding, McpOAuthStateInput,
    McpOAuthStoredCredentials, authorize_mcp_oauth_server,
};
use crate::relay_secrets::RelaySecretManager;

impl super::ConfigRepository {
    async fn require_mcp_oauth_fingerprint(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<String> {
        let server = self
            .get_mcp_server(server_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("mcp server not found"))?;
        authorize_mcp_oauth_server(&server, owner_user_id)
    }

    async fn require_mcp_oauth_owner(&self, server_id: Uuid, owner_user_id: i64) -> Result<()> {
        let server = self
            .get_mcp_server(server_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("mcp server not found"))?;
        anyhow::ensure!(
            server.scope == crate::db::MCP_OAUTH_SUPPORTED_SCOPE,
            "personal MCP OAuth requires a user-scoped server"
        );
        anyhow::ensure!(
            server.owner_user_id == Some(owner_user_id),
            "personal MCP OAuth requires the server owner"
        );
        Ok(())
    }

    /// Load the decryptable credential bundle for the current server config.
    pub async fn load_mcp_oauth_credential(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<Option<McpOAuthCredentialSecrets>> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        let loaded = match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::load_credential(
                    repo.pool(),
                    manager,
                    server_id,
                    owner_user_id,
                )
                .await?
            }
            Self::Sqlite(repo) => {
                repo.store()
                    .load_mcp_oauth_credential(manager, server_id, owner_user_id)
                    .await?
            }
        };
        Ok(loaded.filter(|secrets| secrets.credential.config_fingerprint == fingerprint))
    }

    /// Load only the public credential status for the current server config.
    pub async fn mcp_oauth_credential_status(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<Option<McpOAuthCredential>> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        let status = match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::load_credential_status(repo.pool(), server_id, owner_user_id)
                    .await?
            }
            Self::Sqlite(repo) => {
                repo.store()
                    .load_mcp_oauth_credential_status(server_id, owner_user_id)
                    .await?
            }
        };
        Ok(status.filter(|status| status.config_fingerprint == fingerprint))
    }

    /// Install a fresh callback grant, installing a new connection epoch.
    pub async fn install_mcp_oauth_grant(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        grant: &McpOAuthGrant,
    ) -> Result<McpOAuthCredential> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::install_grant(
                    repo.pool(),
                    manager,
                    server_id,
                    owner_user_id,
                    &fingerprint,
                    grant,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .install_mcp_oauth_grant(manager, server_id, owner_user_id, &fingerprint, grant)
                .await
                .map_err(Into::into),
        }
    }

    /// Save a refresh result through a live lease fence.
    pub async fn save_mcp_oauth_refresh(
        &self,
        manager: &RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        fence: McpOAuthRefreshFence,
        credentials: &McpOAuthStoredCredentials,
    ) -> Result<Option<McpOAuthCredential>> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::save_refresh(
                    repo.pool(),
                    manager,
                    server_id,
                    owner_user_id,
                    &fingerprint,
                    fence,
                    credentials,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .save_mcp_oauth_refresh(
                    manager,
                    server_id,
                    owner_user_id,
                    &fingerprint,
                    fence,
                    credentials,
                )
                .await
                .map_err(Into::into),
        }
    }

    /// Clear the stored credential and every pending state/lease.
    pub async fn clear_mcp_oauth_credential(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
    ) -> Result<bool> {
        self.require_mcp_oauth_owner(server_id, owner_user_id)
            .await?;
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::clear_credential(repo.pool(), server_id, owner_user_id).await
            }
            Self::Sqlite(repo) => repo
                .store()
                .clear_mcp_oauth_credential(server_id, owner_user_id)
                .await
                .map_err(Into::into),
        }
    }

    /// Store one single-use authorization state for the current config.
    pub async fn create_mcp_oauth_state(
        &self,
        manager: &RelaySecretManager,
        input: &McpOAuthStateInput,
    ) -> Result<()> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(input.server_id, input.owner_user_id)
            .await?;
        anyhow::ensure!(
            input.config_fingerprint == fingerprint,
            "MCP OAuth state was created for a stale server configuration"
        );
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::create_state(repo.pool(), manager, input).await
            }
            Self::Sqlite(repo) => repo
                .store()
                .create_mcp_oauth_state(manager, input)
                .await
                .map_err(Into::into),
        }
    }

    /// Whether an unexpired authorization state is still pending for this
    /// server owner under the current configuration.
    ///
    /// Status is derived from this storage fact so every worker agrees, and a
    /// stale binding (different fingerprint) never reports as pending.
    pub async fn has_active_mcp_oauth_state(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        config_fingerprint: &str,
    ) -> Result<bool> {
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::has_active_state(
                    repo.pool(),
                    server_id,
                    owner_user_id,
                    config_fingerprint,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .has_mcp_oauth_state(server_id, owner_user_id, config_fingerprint)
                .await
                .map_err(Into::into),
        }
    }

    /// Read the non-secret binding of an unexpired state row.
    pub async fn peek_mcp_oauth_state(
        &self,
        state_key_hash: &str,
    ) -> Result<Option<McpOAuthStateBinding>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::peek_state(repo.pool(), state_key_hash).await
            }
            Self::Sqlite(repo) => repo
                .store()
                .peek_mcp_oauth_state(state_key_hash)
                .await
                .map_err(Into::into),
        }
    }

    /// Read one unexpired pending authorization without consuming it.
    ///
    /// This is the callback preflight: it enforces the exact state digest, the
    /// expected server, the authenticated owner, the hashed session binding,
    /// the current configuration fingerprint and expiry, and only then
    /// decrypts the pending snapshot for validation.
    pub async fn peek_mcp_oauth_authorization(
        &self,
        manager: &RelaySecretManager,
        state_key_hash: &str,
        server_id: Uuid,
        owner_user_id: i64,
        session_binding_hash: &str,
    ) -> Result<Option<crate::db::mcp_oauth::PendingAuthorizationState>> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::peek_authorization_state(
                    repo.pool(),
                    manager,
                    state_key_hash,
                    server_id,
                    owner_user_id,
                    session_binding_hash,
                    &fingerprint,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .peek_mcp_oauth_authorization(
                    manager,
                    state_key_hash,
                    server_id,
                    owner_user_id,
                    session_binding_hash,
                    &fingerprint,
                )
                .await
                .map_err(Into::into),
        }
    }

    /// Atomically consume one state row when owner, session, config and expiry match.
    pub async fn consume_mcp_oauth_state(
        &self,
        manager: &RelaySecretManager,
        state_key_hash: &str,
        owner_user_id: i64,
        session_binding_hash: &str,
    ) -> Result<Option<McpOAuthConsumedState>> {
        let Some(binding) = self.peek_mcp_oauth_state(state_key_hash).await? else {
            return Ok(None);
        };
        if binding.owner_user_id != owner_user_id {
            return Ok(None);
        }
        let Ok(fingerprint) = self
            .require_mcp_oauth_fingerprint(binding.server_id, owner_user_id)
            .await
        else {
            return Ok(None);
        };
        if binding.config_fingerprint != fingerprint {
            return Ok(None);
        }
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::consume_state(
                    repo.pool(),
                    manager,
                    state_key_hash,
                    owner_user_id,
                    session_binding_hash,
                    &fingerprint,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .consume_mcp_oauth_state(
                    manager,
                    state_key_hash,
                    owner_user_id,
                    session_binding_hash,
                    &fingerprint,
                )
                .await
                .map_err(Into::into),
        }
    }

    /// Claim the short refresh lease for the stored epoch/generation.
    pub async fn claim_mcp_oauth_lease(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        lease_owner: Uuid,
        ttl: Duration,
    ) -> Result<Option<McpOAuthLease>> {
        let fingerprint = self
            .require_mcp_oauth_fingerprint(server_id, owner_user_id)
            .await?;
        let Some(status) = self
            .mcp_oauth_credential_status(server_id, owner_user_id)
            .await?
        else {
            return Ok(None);
        };
        if status.config_fingerprint != fingerprint || status.generation < 1 {
            return Ok(None);
        }
        let expires_at = Utc::now() + ttl;
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::claim_lease(
                    repo.pool(),
                    server_id,
                    owner_user_id,
                    status.connection_id,
                    status.generation,
                    lease_owner,
                    expires_at,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .claim_mcp_oauth_lease(
                    server_id,
                    owner_user_id,
                    status.connection_id,
                    status.generation,
                    lease_owner,
                    expires_at,
                )
                .await
                .map_err(Into::into),
        }
    }

    /// Release a lease only when it still belongs to the caller.
    pub async fn release_mcp_oauth_lease(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        fence: McpOAuthRefreshFence,
    ) -> Result<bool> {
        match self {
            Self::Postgres(repo) => {
                crate::db::mcp_oauth::release_lease(
                    repo.pool(),
                    server_id,
                    owner_user_id,
                    fence.connection_id,
                    fence.generation,
                    fence.lease_owner,
                )
                .await
            }
            Self::Sqlite(repo) => repo
                .store()
                .release_mcp_oauth_lease(
                    server_id,
                    owner_user_id,
                    fence.connection_id,
                    fence.generation,
                    fence.lease_owner,
                )
                .await
                .map_err(Into::into),
        }
    }
}
