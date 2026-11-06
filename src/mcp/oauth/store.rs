//! Storage adapters that bind rmcp's OAuth state machine to the frozen
//! personal-MCP-OAuth persistence boundary.
//!
//! The database layer (P2) owns encryption, the connection epoch/generation
//! fence, the config fingerprint and single-use state consumption. This module
//! only translates between rmcp's engine types and the opaque snapshots that
//! the database stores, and implements the two rmcp store traits on top of the
//! existing repository API. Nothing here interprets a secret.

use std::sync::Mutex;

use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rmcp::transport::auth::{
    AuthorizationMetadata, CredentialRefreshGuard, CredentialStore, StateStore,
    StoredAuthorizationState, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{
    ConfigRepository, MCP_OAUTH_LEASE_TTL_SECONDS, McpOAuthAuthorizationState, McpOAuthGrant,
    McpOAuthRefreshFence, McpOAuthRegistration, McpOAuthStateInput, McpOAuthStoredCredentials,
};
use crate::keys::hash_client_key;
use crate::relay_secrets::RelaySecretManager;

/// One confidential client registration together with the authorization
/// server metadata and the protected resource it was obtained for.
///
/// The whole value is sealed inside the database envelope; it is never exposed
/// through the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RegistrationSnapshot {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// `client_secret_basic` or `client_secret_post`; the restored manager must
    /// use the same method that dynamic registration declared.
    pub client_auth_method: String,
    pub redirect_uri: String,
    /// The RFC 8707 resource indicator, preserved verbatim (including any
    /// `?login` query) so rebuilt managers send the same value.
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub metadata: AuthorizationMetadata,
}

impl RegistrationSnapshot {
    pub(crate) fn to_registration(&self) -> Result<McpOAuthRegistration> {
        Ok(McpOAuthRegistration {
            snapshot: serde_json::to_value(self)?,
        })
    }

    pub(crate) fn from_registration(registration: &McpOAuthRegistration) -> Result<Self> {
        Ok(serde_json::from_value(registration.snapshot.clone())?)
    }
}

/// The token response required `token_type` plus every vendor field; the whole
/// rmcp credential snapshot is stored as one opaque value.
pub(crate) fn encode_credentials(stored: &StoredCredentials) -> Result<McpOAuthStoredCredentials> {
    let engine = serde_json::to_value(stored)?;
    let token = engine.get("token_response");
    let expires_in = token
        .and_then(|token| token.get("expires_in"))
        .and_then(serde_json::Value::as_u64);
    let received_at = stored.token_received_at;
    let expires_at = match (expires_in, received_at) {
        (Some(expires_in), Some(received_at)) => {
            let received = DateTime::<Utc>::from_timestamp(received_at as i64, 0);
            received.map(|received| received + ChronoDuration::seconds(expires_in as i64))
        }
        _ => None,
    };
    Ok(McpOAuthStoredCredentials {
        engine,
        scopes: stored.granted_scopes.clone(),
        expires_at,
        issuer: stored.issuer.clone(),
    })
}

pub(crate) fn decode_credentials(
    credentials: &McpOAuthStoredCredentials,
) -> Result<StoredCredentials> {
    Ok(serde_json::from_value(credentials.engine.clone())?)
}

/// The access token currently held in a stored snapshot, when one is present.
pub(crate) fn access_token(credentials: &McpOAuthStoredCredentials) -> Option<String> {
    credentials
        .engine
        .get("token_response")
        .and_then(|token| token.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// A snapshot that keeps the client identity and scopes but no usable token,
/// written when the authorization server definitively rejected the refresh
/// token. It keeps the row (so the status is `reauth_required`) instead of
/// silently discarding the grant or leaking a stale token.
pub(crate) fn rejected_credentials(
    stored: &StoredCredentials,
) -> Result<McpOAuthStoredCredentials> {
    let engine = serde_json::json!({
        "client_id": stored.client_id,
        "token_response": serde_json::Value::Null,
        "granted_scopes": stored.granted_scopes,
        "token_received_at": serde_json::Value::Null,
        "issuer": stored.issuer,
    });
    Ok(McpOAuthStoredCredentials {
        engine,
        scopes: stored.granted_scopes.clone(),
        expires_at: None,
        issuer: stored.issuer.clone(),
    })
}

pub(crate) fn encode_authorization(
    state: &StoredAuthorizationState,
) -> Result<McpOAuthAuthorizationState> {
    Ok(McpOAuthAuthorizationState {
        snapshot: serde_json::to_value(state)?,
    })
}

pub(crate) fn decode_authorization(
    state: &McpOAuthAuthorizationState,
) -> Result<StoredAuthorizationState> {
    Ok(serde_json::from_value(state.snapshot.clone())?)
}

/// How a credential store persists what rmcp hands it.
pub(crate) enum CredentialStoreMode {
    /// Capture the credentials produced by a code exchange. The flow validates
    /// them and installs them as a fresh connection epoch itself.
    Capture,
    /// Persist a refresh result through the live database lease fence.
    Refresh,
}

pub(crate) struct OAuthCredentialStoreInner {
    repository: ConfigRepository,
    secrets: RelaySecretManager,
    server_id: Uuid,
    owner_user_id: i64,
    mode: CredentialStoreMode,
    captured: Mutex<Option<StoredCredentials>>,
    fence: Mutex<Option<McpOAuthRefreshFence>>,
}

/// Cloneable handle to one credential store; the manager owns one clone while
/// the flow keeps another to read the captured grant or the refresh fence.
#[derive(Clone)]
pub(crate) struct OAuthCredentialStore {
    inner: std::sync::Arc<OAuthCredentialStoreInner>,
}

impl OAuthCredentialStore {
    pub(crate) fn new(
        repository: ConfigRepository,
        secrets: RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        mode: CredentialStoreMode,
    ) -> Self {
        Self {
            inner: std::sync::Arc::new(OAuthCredentialStoreInner {
                repository,
                secrets,
                server_id,
                owner_user_id,
                mode,
                captured: Mutex::new(None),
                fence: Mutex::new(None),
            }),
        }
    }

    fn repository(&self) -> &ConfigRepository {
        &self.inner.repository
    }
    fn secrets(&self) -> &RelaySecretManager {
        &self.inner.secrets
    }
    fn server_id(&self) -> Uuid {
        self.inner.server_id
    }
    fn owner_user_id(&self) -> i64 {
        self.inner.owner_user_id
    }
    fn mode(&self) -> &CredentialStoreMode {
        &self.inner.mode
    }

    pub(crate) fn take_captured(&self) -> Option<StoredCredentials> {
        self.inner
            .captured
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    pub(crate) fn take_fence(&self) -> Option<McpOAuthRefreshFence> {
        self.inner
            .fence
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    fn current_fence(&self) -> Result<McpOAuthRefreshFence> {
        self.inner
            .fence
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .ok_or_else(|| anyhow::anyhow!("refresh lease is not held"))
    }
}

#[async_trait::async_trait]
impl CredentialStore for OAuthCredentialStore {
    async fn load(
        &self,
    ) -> std::result::Result<Option<StoredCredentials>, rmcp::transport::auth::AuthError> {
        match self.mode() {
            CredentialStoreMode::Capture => Ok(None),
            CredentialStoreMode::Refresh => {
                let loaded = self
                    .repository()
                    .load_mcp_oauth_credential(
                        self.secrets(),
                        self.server_id(),
                        self.owner_user_id(),
                    )
                    .await
                    .map_err(store_error)?;
                loaded
                    .as_ref()
                    .map(|secrets| decode_credentials(&secrets.credentials))
                    .transpose()
                    .map_err(store_error)
            }
        }
    }

    async fn save(
        &self,
        credentials: StoredCredentials,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        match self.mode() {
            CredentialStoreMode::Capture => {
                if let Ok(mut slot) = self.inner.captured.lock() {
                    *slot = Some(credentials);
                }
                Ok(())
            }
            CredentialStoreMode::Refresh => {
                let fence = self.current_fence().map_err(store_error)?;
                validate_stored_scopes(&credentials)
                    .map_err(rmcp::transport::auth::AuthError::CredentialStoreError)?;
                let encoded = encode_credentials(&credentials).map_err(store_error)?;
                let saved = self
                    .repository()
                    .save_mcp_oauth_refresh(
                        self.secrets(),
                        self.server_id(),
                        self.owner_user_id(),
                        fence,
                        &encoded,
                    )
                    .await
                    .map_err(store_error)?;
                if saved.is_none() {
                    bail_store("the stored grant changed while the token was being refreshed")?;
                }
                Ok(())
            }
        }
    }

    async fn clear(&self) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        self.repository()
            .clear_mcp_oauth_credential(self.server_id(), self.owner_user_id())
            .await
            .map_err(store_error)?;
        Ok(())
    }

    async fn on_refresh_token_rejected(
        &self,
        credentials: &StoredCredentials,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        if !matches!(self.mode(), CredentialStoreMode::Refresh) {
            return Ok(());
        }
        let Ok(fence) = self.current_fence() else {
            return Ok(());
        };
        // Only clear the token we actually refreshed: a newer grant installed
        // while this refresh was in flight must survive untouched.
        let current = self
            .repository()
            .mcp_oauth_credential_status(self.server_id(), self.owner_user_id())
            .await
            .map_err(store_error)?;
        let Some(current) = current else {
            return Ok(());
        };
        if current.connection_id != fence.connection_id || current.generation != fence.generation {
            return Ok(());
        }
        let rejected = rejected_credentials(credentials).map_err(store_error)?;
        self.repository()
            .save_mcp_oauth_refresh(
                self.secrets(),
                self.server_id(),
                self.owner_user_id(),
                fence,
                &rejected,
            )
            .await
            .map_err(store_error)?;
        Ok(())
    }

    async fn acquire_refresh_guard(
        &self,
    ) -> std::result::Result<Option<CredentialRefreshGuard>, rmcp::transport::auth::AuthError> {
        if !matches!(self.mode(), CredentialStoreMode::Refresh) {
            return Ok(None);
        }
        // Bounded wait: a concurrent refresh holds the lease for up to its
        // 60s lifetime; wait a short while so the caller can reload the
        // result the other writer installed instead of double-refreshing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let lease_owner = Uuid::new_v4();
            let claimed = self
                .repository()
                .claim_mcp_oauth_lease(
                    self.server_id(),
                    self.owner_user_id(),
                    lease_owner,
                    chrono::Duration::seconds(MCP_OAUTH_LEASE_TTL_SECONDS),
                )
                .await
                .map_err(store_error)?;
            if let Some(lease) = claimed {
                let fence = McpOAuthRefreshFence {
                    connection_id: lease.connection_id,
                    generation: lease.generation,
                    lease_owner: lease.lease_owner,
                };
                if let Ok(mut slot) = self.inner.fence.lock() {
                    *slot = Some(fence);
                }
                return Ok(Some(CredentialRefreshGuard::new(fence)));
            }
            if std::time::Instant::now() >= deadline {
                bail_store("another refresh is already in progress")?;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

fn store_error(error: anyhow::Error) -> rmcp::transport::auth::AuthError {
    rmcp::transport::auth::AuthError::CredentialStoreError(error.to_string())
}

fn bail_store(message: &str) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
    Err(rmcp::transport::auth::AuthError::CredentialStoreError(
        message.to_string(),
    ))
}

/// Reject any granted scope beyond the single read-only scope the client asks
/// for. Initial authorization and every refresh share this one check so a
/// widened provider response cannot be persisted or returned.
pub(crate) fn validate_stored_scopes(credentials: &StoredCredentials) -> Result<(), String> {
    if credentials
        .granted_scopes
        .iter()
        .any(|scope| scope != super::MCP_OAUTH_SCOPE)
    {
        return Err(
            "the authorization server granted scopes beyond the requested read-only scope"
                .to_string(),
        );
    }
    Ok(())
}

/// Persists `get_authorization_url` state for a start request.
pub(crate) struct OAuthPendingStateStoreInner {
    repository: ConfigRepository,
    secrets: RelaySecretManager,
    server_id: Uuid,
    owner_user_id: i64,
    session_binding_hash: String,
    connection_id: Uuid,
    config_fingerprint: String,
    redirect_uri: String,
    saved: Mutex<Option<(String, StoredAuthorizationState)>>,
    pending_client: Mutex<Option<McpOAuthRegistration>>,
}

/// Cloneable handle to the pending-authorization state store; the manager owns
/// one clone while the flow attaches the client registration afterwards.
#[derive(Clone)]
pub(crate) struct OAuthPendingStateStore {
    inner: std::sync::Arc<OAuthPendingStateStoreInner>,
}

impl OAuthPendingStateStore {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        repository: ConfigRepository,
        secrets: RelaySecretManager,
        server_id: Uuid,
        owner_user_id: i64,
        session_binding_hash: String,
        connection_id: Uuid,
        config_fingerprint: String,
        redirect_uri: String,
    ) -> Self {
        Self {
            inner: std::sync::Arc::new(OAuthPendingStateStoreInner {
                repository,
                secrets,
                server_id,
                owner_user_id,
                session_binding_hash,
                connection_id,
                config_fingerprint,
                redirect_uri,
                saved: Mutex::new(None),
                pending_client: Mutex::new(None),
            }),
        }
    }

    fn input(
        &self,
        state_key_hash: String,
        authorization: McpOAuthAuthorizationState,
        pending_client: Option<McpOAuthRegistration>,
    ) -> McpOAuthStateInput {
        McpOAuthStateInput {
            state_key_hash,
            session_binding_hash: self.inner.session_binding_hash.clone(),
            server_id: self.inner.server_id,
            owner_user_id: self.inner.owner_user_id,
            connection_id: self.inner.connection_id,
            config_fingerprint: self.inner.config_fingerprint.clone(),
            redirect_uri: self.inner.redirect_uri.clone(),
            expires_at: Utc::now()
                + ChronoDuration::seconds(crate::db::MCP_OAUTH_STATE_TTL_SECONDS),
            authorization,
            pending_client,
        }
    }

    /// Attach the confidential client registration to the already saved state.
    /// `get_authorization_url` saves before the flow knows the resource
    /// indicator, so the row is upserted once more with the complete snapshot.
    pub(crate) async fn set_pending_client(
        &self,
        registration: McpOAuthRegistration,
    ) -> Result<()> {
        if let Ok(mut slot) = self.inner.pending_client.lock() {
            *slot = Some(registration.clone());
        }
        let saved = self.inner.saved.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|(hash, state)| (hash.clone(), state.clone()))
        });
        if let Some((state_key_hash, state)) = saved {
            let authorization = encode_authorization(&state)?;
            self.inner
                .repository
                .create_mcp_oauth_state(
                    &self.inner.secrets,
                    &self.input(state_key_hash, authorization, Some(registration)),
                )
                .await?;
        }
        Ok(())
    }

    fn pending_client(&self) -> Option<McpOAuthRegistration> {
        self.inner
            .pending_client
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }
}

#[async_trait::async_trait]
impl StateStore for OAuthPendingStateStore {
    async fn save(
        &self,
        csrf_token: &str,
        state: StoredAuthorizationState,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        let state_key_hash = hash_client_key(csrf_token);
        let authorization = encode_authorization(&state).map_err(store_error)?;
        self.inner
            .repository
            .create_mcp_oauth_state(
                &self.inner.secrets,
                &self.input(state_key_hash.clone(), authorization, self.pending_client()),
            )
            .await
            .map_err(store_error)?;
        if let Ok(mut slot) = self.inner.saved.lock() {
            *slot = Some((state_key_hash, state));
        }
        Ok(())
    }

    async fn load(
        &self,
        _csrf_token: &str,
    ) -> std::result::Result<Option<StoredAuthorizationState>, rmcp::transport::auth::AuthError>
    {
        Ok(None)
    }

    async fn delete(
        &self,
        _csrf_token: &str,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        Ok(())
    }
}

/// One-shot state store handed to a rebuilt manager for the callback exchange.
///
/// The database row was already consumed atomically (with owner, session,
/// fingerprint and expiry checks) before this store is built, so the exchange
/// can only ever run once and there is no load/delete race between concurrent
/// callbacks.
pub(crate) struct OAuthConsumedStateStore {
    state: Mutex<Option<StoredAuthorizationState>>,
}

impl OAuthConsumedStateStore {
    pub(crate) fn new(state: StoredAuthorizationState) -> Self {
        Self {
            state: Mutex::new(Some(state)),
        }
    }
}

#[async_trait::async_trait]
impl StateStore for OAuthConsumedStateStore {
    async fn save(
        &self,
        _csrf_token: &str,
        _state: StoredAuthorizationState,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        bail_store("authorization state is already consumed")
    }

    async fn load(
        &self,
        _csrf_token: &str,
    ) -> std::result::Result<Option<StoredAuthorizationState>, rmcp::transport::auth::AuthError>
    {
        Ok(self.state.lock().ok().and_then(|mut slot| slot.take()))
    }

    async fn delete(
        &self,
        _csrf_token: &str,
    ) -> std::result::Result<(), rmcp::transport::auth::AuthError> {
        Ok(())
    }
}

/// Build the rmcp client configuration that matches a stored registration.
pub(crate) fn client_config(
    registration: &RegistrationSnapshot,
) -> rmcp::transport::auth::OAuthClientConfig {
    let config = rmcp::transport::auth::OAuthClientConfig::new(
        registration.client_id.clone(),
        registration.redirect_uri.clone(),
    )
    .with_scopes(vec![super::MCP_OAUTH_SCOPE.to_string()])
    .with_application_type("web");
    match registration.client_secret.clone() {
        Some(secret) => config.with_client_secret(secret),
        None => config,
    }
}

/// The credential bundle a callback installs as a fresh connection epoch.
pub(crate) fn grant(
    registration: &RegistrationSnapshot,
    credentials: &StoredCredentials,
) -> Result<McpOAuthGrant> {
    Ok(McpOAuthGrant {
        registration: registration.to_registration()?,
        credentials: encode_credentials(credentials)?,
    })
}
