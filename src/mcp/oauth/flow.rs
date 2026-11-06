//! Authorization, callback, refresh and disconnect logic for personal MCP
//! OAuth.
//!
//! Every operation revalidates the owner instead of trusting a cached session
//! snapshot: the exact server owner, the live browser session binding and a
//! fresh active-user lookup must all hold before any external request. A
//! deleted or deactivated owner fails closed without touching the network.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationMetadata, OAuthClientConfig, OAuthHttpClient,
    StoredCredentials,
};
use uuid::Uuid;

use super::MCP_OAUTH_SCOPE;
use super::http::PinnedOAuthHttpClient;
use super::store::{
    CredentialStoreMode, OAuthConsumedStateStore, OAuthCredentialStore, OAuthPendingStateStore,
    RegistrationSnapshot, access_token, client_config, decode_authorization, grant,
};
use crate::db::{
    ConfigRepository, MCP_OAUTH_SUPPORTED_TRANSPORT, McpOAuthCredential, McpServer, UserStore,
    authorize_mcp_oauth_server,
};
use crate::keys::hash_client_key;
use crate::relay_secrets::RelaySecretManager;

/// Refresh a token this many seconds before it expires.
const REFRESH_SKEW_SECONDS: i64 = 30;

/// Public status of one server's personal OAuth grant. It never carries a
/// token, client secret, PKCE verifier or callback code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpOAuthState {
    Connected,
    Authorizing,
    ReauthRequired,
    Disconnected,
}

impl McpOAuthState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Authorizing => "authorizing",
            Self::ReauthRequired => "reauth_required",
            Self::Disconnected => "disconnected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpOAuthStatus {
    pub state: McpOAuthState,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    /// A safe, actionable explanation of why OAuth is unavailable, when the
    /// configuration itself is incomplete.
    pub reason: Option<String>,
}

/// Why one OAuth operation did not succeed. The variants map onto stable API
/// error codes; none of them carries a token, client secret or provider body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum McpOAuthError {
    /// The server is not eligible for personal OAuth, or the deployment is
    /// missing a required setting. Only OAuth for this server is disabled.
    Disabled(String),
    /// The caller is not the server owner or the owner is no longer active.
    Forbidden,
    /// The server does not exist or is not visible to the caller.
    NotFound,
    /// The callback state is unknown, expired, replayed or bound elsewhere.
    InvalidState,
    /// A new sign-in is required.
    AuthorizationRequired,
    /// Another refresh is already running; retry shortly.
    RefreshInProgress,
    /// The relay secret manager is not configured.
    Unavailable,
    /// An upstream authorization-server interaction failed.
    Upstream(String),
}

impl McpOAuthError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Disabled(_) => "mcp_oauth_disabled",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::InvalidState => "mcp_oauth_invalid_state",
            Self::AuthorizationRequired => "mcp_oauth_reauthorization_required",
            Self::RefreshInProgress => "mcp_oauth_refresh_in_progress",
            Self::Unavailable => "mcp_oauth_unavailable",
            Self::Upstream(_) => "mcp_oauth_upstream_error",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Disabled(reason) => reason.clone(),
            Self::Forbidden => "only the server owner may use personal MCP OAuth".to_string(),
            Self::NotFound => "mcp server not found".to_string(),
            Self::InvalidState => "the authorization request is invalid or expired".to_string(),
            Self::AuthorizationRequired => "sign in to the MCP server again".to_string(),
            Self::RefreshInProgress => "a token refresh is already in progress".to_string(),
            Self::Unavailable => "the relay secret manager is not configured".to_string(),
            Self::Upstream(reason) => reason.clone(),
        }
    }
}

/// The injected personal MCP OAuth backend.
#[derive(Clone)]
pub(crate) struct McpOAuthService {
    repository: ConfigRepository,
    secrets: Option<RelaySecretManager>,
    redirect_base: Option<String>,
    user_store: UserStore,
    /// Deployment-level reason personal OAuth is unavailable, surfaced by the
    /// status API and every operation instead of using an unsafe fallback.
    disabled_reason: Option<String>,
    /// Test-only injection for a loopback mock authorization server. Compiled
    /// only for tests; no deployment flag can enable it.
    #[cfg(test)]
    allow_private_network: bool,
    /// Test-only short timeout cap for the bounded-time regression.
    #[cfg(test)]
    oauth_timeout: Option<std::time::Duration>,
    /// Test-only whole-operation budget, defaulting to the production 30s cap.
    #[cfg(test)]
    operation_timeout: Option<std::time::Duration>,
    /// Test-only switch that keeps discovery on loopback while applying the
    /// strict production browser-endpoint policy under test.
    #[cfg(test)]
    strict_browser_policy: bool,
}

struct Settings {
    secrets: RelaySecretManager,
    redirect_uri: String,
    server_url: String,
    fingerprint: String,
}

impl McpOAuthService {
    pub(crate) fn new(
        repository: ConfigRepository,
        secrets: Option<RelaySecretManager>,
        redirect_base: Option<String>,
        user_store: UserStore,
    ) -> Self {
        Self {
            repository,
            secrets,
            redirect_base: redirect_base.filter(|value| !value.trim().is_empty()),
            user_store,
            disabled_reason: None,
            #[cfg(test)]
            allow_private_network: false,
            #[cfg(test)]
            oauth_timeout: None,
            #[cfg(test)]
            operation_timeout: None,
            #[cfg(test)]
            strict_browser_policy: false,
        }
    }

    /// Test-only: allow the loopback mock authorization server.
    #[cfg(test)]
    pub(crate) fn with_private_network(mut self) -> Self {
        self.allow_private_network = true;
        self
    }

    /// Test-only: same loopback mock, with a short timeout cap.
    #[cfg(test)]
    pub(crate) fn with_short_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.allow_private_network = true;
        self.oauth_timeout = Some(timeout);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_operation_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.operation_timeout = Some(timeout);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_strict_browser_policy(mut self) -> Self {
        self.strict_browser_policy = true;
        self
    }

    fn operation_budget(&self) -> std::time::Duration {
        #[cfg(test)]
        let budget = self
            .operation_timeout
            .unwrap_or(std::time::Duration::from_secs(30));
        #[cfg(not(test))]
        let budget = std::time::Duration::from_secs(30);
        budget
    }

    /// Mark personal OAuth unavailable for a deployment-level reason while
    /// keeping the rest of the worker functional.
    pub(crate) fn with_disabled_reason(mut self, reason: impl Into<String>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    #[cfg(test)]
    pub(crate) fn disabled_reason(&self) -> Option<&str> {
        self.disabled_reason.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn allows_test_network(&self) -> bool {
        self.allow_private_network
    }

    fn http_client(&self) -> Arc<PinnedOAuthHttpClient> {
        #[cfg(test)]
        if self.allow_private_network {
            return Arc::new(match self.oauth_timeout {
                Some(timeout) => PinnedOAuthHttpClient::for_test_with_timeout(timeout),
                None => PinnedOAuthHttpClient::for_test(),
            });
        }
        Arc::new(PinnedOAuthHttpClient::new())
    }

    fn secrets(&self) -> Result<&RelaySecretManager, McpOAuthError> {
        if let Some(reason) = self.disabled_reason.as_deref() {
            return Err(McpOAuthError::Disabled(reason.to_string()));
        }
        self.secrets.as_ref().ok_or(McpOAuthError::Unavailable)
    }

    /// Validate the server, deployment settings and the owner's current
    /// activity. Everything that follows relies on this having succeeded.
    async fn settings(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<Settings, McpOAuthError> {
        // Ownership is a hard authorization boundary: another user (admin
        // included) is forbidden, while a server that simply is not a personal
        // OAuth server is reported as disabled.
        if server.scope != crate::db::MCP_OAUTH_SUPPORTED_SCOPE
            || server.owner_user_id != Some(owner_user_id)
        {
            return Err(McpOAuthError::Forbidden);
        }
        let fingerprint = authorize_mcp_oauth_server(server, owner_user_id)
            .map_err(|error| McpOAuthError::Disabled(error.to_string()))?;
        if server.transport != MCP_OAUTH_SUPPORTED_TRANSPORT {
            return Err(McpOAuthError::Disabled(
                "personal MCP OAuth supports only the http transport".to_string(),
            ));
        }
        if server
            .proxy_url
            .as_deref()
            .is_some_and(|proxy| !proxy.trim().is_empty())
        {
            return Err(McpOAuthError::Disabled(
                "personal MCP OAuth does not support a per-server proxy".to_string(),
            ));
        }
        let server_url = server
            .url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .ok_or_else(|| {
                McpOAuthError::Disabled(
                    "personal MCP OAuth requires an HTTPS server URL".to_string(),
                )
            })?;
        let parsed = reqwest::Url::parse(server_url).map_err(|_| {
            McpOAuthError::Disabled("the MCP server URL is not a valid URL".to_string())
        })?;
        #[cfg(not(test))]
        let require_https = true;
        #[cfg(test)]
        let require_https = !self.allow_private_network;
        if require_https && parsed.scheme() != "https" {
            return Err(McpOAuthError::Disabled(
                "personal MCP OAuth requires an HTTPS server URL".to_string(),
            ));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(McpOAuthError::Disabled(
                "the MCP server URL must not embed credentials".to_string(),
            ));
        }
        let redirect_base = self.redirect_base.as_deref().ok_or_else(|| {
            McpOAuthError::Disabled(
                "set mcp_oauth_redirect_base to the public HTTPS base URL that serves the callback"
                    .to_string(),
            )
        })?;
        validate_redirect_base(redirect_base)?;
        let redirect_uri = format!(
            "{}/api/v1/admin/mcp-servers/{}/oauth/callback",
            redirect_base.trim_end_matches('/'),
            server.server_id
        );
        let secrets = self.secrets()?.clone();
        let active = self
            .user_store
            .get_active_user(owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(format!("owner lookup failed: {error}")))?;
        if active.is_none() {
            return Err(McpOAuthError::Forbidden);
        }
        Ok(Settings {
            secrets,
            redirect_uri,
            server_url: server_url.to_string(),
            fingerprint,
        })
    }

    /// Validate an authorization-server browser destination before it is used
    /// or handed to the owner. Production requires a public HTTPS endpoint and
    /// resolves every address once; the test-only loopback seam still rejects
    /// credentials, fragments and non-HTTP(S) schemes.
    async fn validate_browser_endpoint(&self, url: &str) -> Result<(), McpOAuthError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| {
            McpOAuthError::Disabled(
                "the authorization server authorization endpoint is not a valid URL".to_string(),
            )
        })?;
        #[cfg(test)]
        if self.allow_private_network && !self.strict_browser_policy {
            if !matches!(parsed.scheme(), "http" | "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.fragment().is_some()
                || parsed.host_str().filter(|host| !host.is_empty()).is_none()
            {
                return Err(McpOAuthError::Disabled(
                    "the authorization server authorization endpoint must be an HTTPS public endpoint"
                        .to_string(),
                ));
            }
            return Ok(());
        }
        crate::mcp::transport::pinning::validate_https_url(&parsed).map_err(|error| {
            McpOAuthError::Disabled(format!(
                "the authorization server authorization endpoint must be a public HTTPS endpoint: {error}"
            ))
        })?;
        let host = parsed.host_str().ok_or_else(|| {
            McpOAuthError::Disabled(
                "the authorization server authorization endpoint must be a public HTTPS endpoint"
                    .to_string(),
            )
        })?;
        let port = parsed.port_or_known_default().unwrap_or(443);
        crate::mcp::transport::pinning::resolve_and_validate(host, port)
            .await
            .map_err(|error| {
                McpOAuthError::Upstream(format!(
                    "failed to resolve the authorization endpoint: {error}"
                ))
            })?
            .map_err(|error| {
                McpOAuthError::Disabled(format!(
                    "the authorization server authorization endpoint must be a public HTTPS endpoint: {error}"
                ))
            })?;
        Ok(())
    }

    /// Begin an authorization. Returns the URL the owner must visit.
    pub(crate) async fn start(
        &self,
        server: &McpServer,
        owner_user_id: i64,
        session_binding_hash: &str,
    ) -> Result<String, McpOAuthError> {
        tokio::time::timeout(
            self.operation_budget(),
            self.start_inner(server, owner_user_id, session_binding_hash),
        )
        .await
        .map_err(|_| {
            McpOAuthError::Upstream("personal MCP OAuth authorization start timed out".to_string())
        })?
    }

    async fn start_inner(
        &self,
        server: &McpServer,
        owner_user_id: i64,
        session_binding_hash: &str,
    ) -> Result<String, McpOAuthError> {
        let settings = self.settings(server, owner_user_id).await?;
        let pinned = self.http_client();
        let http: Arc<dyn OAuthHttpClient> = pinned.clone();
        let mut manager =
            AuthorizationManager::new_with_oauth_http_client(settings.server_url.clone(), http)
                .await
                .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;

        let resolution = manager
            .resolve_metadata()
            .await
            .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        let metadata = resolution.metadata;
        self.validate_browser_endpoint(&metadata.authorization_endpoint)
            .await?;
        let client_auth_method = choose_auth_method(&metadata)?;
        let registration_endpoint = metadata.registration_endpoint.clone().ok_or_else(|| {
            McpOAuthError::Disabled(
                "the authorization server does not support dynamic client registration".to_string(),
            )
        })?;
        let (client_id, client_secret) = register_client(
            &pinned,
            &registration_endpoint,
            &settings.redirect_uri,
            client_auth_method,
        )
        .await?;

        manager.set_metadata(metadata.clone());
        let config = OAuthClientConfig::new(client_id.clone(), settings.redirect_uri.clone())
            .with_client_secret(client_secret.clone())
            .with_scopes(vec![MCP_OAUTH_SCOPE.to_string()])
            .with_application_type("web");
        manager
            .configure_client(config)
            .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;

        let state_store = OAuthPendingStateStore::new(
            self.repository.clone(),
            settings.secrets.clone(),
            server.server_id,
            owner_user_id,
            session_binding_hash.to_string(),
            Uuid::new_v4(),
            settings.fingerprint.clone(),
            settings.redirect_uri.clone(),
        );
        manager.set_state_store(state_store.clone());

        let authorization_url = manager
            .get_authorization_url(&[MCP_OAUTH_SCOPE])
            .await
            .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        self.validate_browser_endpoint(&authorization_url).await?;
        let resource = authorization_resource(&authorization_url)
            .unwrap_or_else(|| settings.server_url.clone());

        let registration = RegistrationSnapshot {
            client_id,
            client_secret: Some(client_secret),
            client_auth_method: client_auth_method.to_string(),
            redirect_uri: settings.redirect_uri.clone(),
            resource,
            issuer: metadata.issuer.clone(),
            metadata,
        };
        let registration = registration
            .to_registration()
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        state_store
            .set_pending_client(registration)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;

        tracing::info!(
            category = "mcp_oauth",
            event = "authorization_started",
            server_id = %server.server_id,
            owner_user_id,
            "personal MCP OAuth authorization started"
        );
        Ok(authorization_url)
    }

    /// Complete an authorization from the callback query parameters.
    pub(crate) async fn callback(
        &self,
        server: &McpServer,
        owner_user_id: i64,
        session_binding_hash: &str,
        code: &str,
        state: &str,
        iss: Option<&str>,
    ) -> Result<(), McpOAuthError> {
        tokio::time::timeout(
            self.operation_budget(),
            self.callback_inner(
                server,
                owner_user_id,
                session_binding_hash,
                code,
                state,
                iss,
            ),
        )
        .await
        .map_err(|_| McpOAuthError::Upstream("personal MCP OAuth callback timed out".to_string()))?
    }

    async fn callback_inner(
        &self,
        server: &McpServer,
        owner_user_id: i64,
        session_binding_hash: &str,
        code: &str,
        state: &str,
        iss: Option<&str>,
    ) -> Result<(), McpOAuthError> {
        // A rejected attempt before consumption leaves its state row intact
        // and therefore keeps reporting `authorizing` until it expires or is
        // consumed; this flow never deletes another valid attempt.
        self.exchange_authorization(
            server,
            owner_user_id,
            session_binding_hash,
            code,
            state,
            iss,
        )
        .await
    }

    async fn exchange_authorization(
        &self,
        server: &McpServer,
        owner_user_id: i64,
        session_binding_hash: &str,
        code: &str,
        state: &str,
        iss: Option<&str>,
    ) -> Result<(), McpOAuthError> {
        let settings = self.settings(server, owner_user_id).await?;
        if code.trim().is_empty() || state.trim().is_empty() {
            return Err(McpOAuthError::InvalidState);
        }
        let state_key_hash = hash_client_key(state);

        // Read-only preflight. It enforces the exact digest, this server, the
        // authenticated owner, the hashed session binding, the current
        // configuration fingerprint and expiry, then validates the stored
        // issuer expectations before anything is consumed. A rejection here
        // leaves the attempt usable until it expires.
        let pending = self
            .repository
            .peek_mcp_oauth_authorization(
                &settings.secrets,
                &state_key_hash,
                server.server_id,
                owner_user_id,
                session_binding_hash,
            )
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::InvalidState)?;
        if pending.binding.server_id != server.server_id {
            return Err(McpOAuthError::InvalidState);
        }
        let pending_registration = pending
            .pending_client
            .as_ref()
            .ok_or(McpOAuthError::InvalidState)?;
        let pending_registration = RegistrationSnapshot::from_registration(pending_registration)
            .map_err(|_| McpOAuthError::InvalidState)?;
        verify_auth_method(&pending_registration)?;
        let pending_authorization = decode_authorization(&pending.authorization)
            .map_err(|_| McpOAuthError::InvalidState)?;
        validate_callback_issuer(
            &pending_authorization,
            pending_registration.issuer.as_deref(),
            iss,
        )?;

        // Atomic single use: the row is deleted only when the owner, session
        // binding, config fingerprint and expiry all still match. Only the
        // winner of this consume may exchange the code, so a concurrent
        // callback that lost the race never reaches the token endpoint.
        let consumed = self
            .repository
            .consume_mcp_oauth_state(
                &settings.secrets,
                &state_key_hash,
                owner_user_id,
                session_binding_hash,
            )
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::InvalidState)?;
        if consumed.binding.server_id != server.server_id {
            return Err(McpOAuthError::InvalidState);
        }
        let authorization = decode_authorization(&consumed.authorization)
            .map_err(|_| McpOAuthError::InvalidState)?;
        let pending = consumed
            .pending_client
            .as_ref()
            .ok_or(McpOAuthError::InvalidState)?;
        let registration = RegistrationSnapshot::from_registration(pending)
            .map_err(|_| McpOAuthError::InvalidState)?;
        verify_auth_method(&registration)?;
        // Re-validate the consumed snapshot: the preflight and the consume must
        // agree on the same authorization material.
        validate_callback_issuer(&authorization, registration.issuer.as_deref(), iss)?;

        let http: Arc<dyn OAuthHttpClient> = self.http_client();
        // The manager is rebuilt from the encrypted registration and metadata,
        // with no network discovery; using the stored resource keeps the
        // RFC 8707 indicator identical to the one sent at authorization time.
        let mut manager =
            AuthorizationManager::new_with_oauth_http_client(registration.resource.clone(), http)
                .await
                .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        manager.set_metadata(registration.metadata.clone());
        manager
            .configure_client(client_config(&registration))
            .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        manager.set_state_store(OAuthConsumedStateStore::new(authorization));
        let credential_store = OAuthCredentialStore::new(
            self.repository.clone(),
            settings.secrets.clone(),
            server.server_id,
            owner_user_id,
            CredentialStoreMode::Capture,
        );
        manager.set_credential_store(credential_store.clone());

        manager
            .exchange_code_for_token_with_issuer(code, state, iss)
            .await
            .map_err(|error| match error {
                AuthError::AuthorizationServerMismatch { .. }
                | AuthError::AuthorizationServerMissingIssuer { .. } => McpOAuthError::InvalidState,
                other => McpOAuthError::Upstream(describe(&other)),
            })?;

        let captured = credential_store.take_captured().ok_or_else(|| {
            McpOAuthError::Upstream("token exchange produced no credentials".into())
        })?;
        validate_granted_scopes(&captured)?;
        let grant = grant(&registration, &captured)
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        self.repository
            .install_mcp_oauth_grant(&settings.secrets, server.server_id, owner_user_id, &grant)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        tracing::info!(
            category = "mcp_oauth",
            event = "authorization_completed",
            server_id = %server.server_id,
            owner_user_id,
            "personal MCP OAuth authorization completed"
        );
        Ok(())
    }

    /// Perform one bounded refresh for the owner's grant.
    pub(crate) async fn refresh(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<McpOAuthCredential, McpOAuthError> {
        tokio::time::timeout(
            self.operation_budget(),
            self.refresh_inner(server, owner_user_id),
        )
        .await
        .map_err(|_| {
            McpOAuthError::Upstream("personal MCP OAuth token refresh timed out".to_string())
        })?
    }

    async fn refresh_inner(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<McpOAuthCredential, McpOAuthError> {
        let settings = self.settings(server, owner_user_id).await?;
        let loaded = self
            .repository
            .load_mcp_oauth_credential(&settings.secrets, server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        let registration = RegistrationSnapshot::from_registration(&loaded.registration)
            .map_err(|_| McpOAuthError::AuthorizationRequired)?;
        verify_auth_method(&registration)?;
        let store = OAuthCredentialStore::new(
            self.repository.clone(),
            settings.secrets.clone(),
            server.server_id,
            owner_user_id,
            CredentialStoreMode::Refresh,
        );
        let http: Arc<dyn OAuthHttpClient> = self.http_client();
        let mut manager =
            AuthorizationManager::new_with_oauth_http_client(registration.resource.clone(), http)
                .await
                .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        let mut refresh_metadata = registration.metadata.clone();
        // rmcp appends offline_access whenever raw provider metadata advertises
        // it. Refresh is authorized for read-mcp only, so the metadata handed
        // to the manager is constrained to that scope while endpoints, issuer
        // and confidential client settings stay intact.
        refresh_metadata.scopes_supported = Some(vec![MCP_OAUTH_SCOPE.to_string()]);
        manager.set_metadata(refresh_metadata);
        manager
            .configure_client(client_config(&registration))
            .map_err(|error| McpOAuthError::Upstream(describe(&error)))?;
        manager.set_credential_store(store.clone());

        let result = manager.refresh_token().await;
        if let Some(fence) = store.take_fence() {
            let _ = self
                .repository
                .release_mcp_oauth_lease(server.server_id, owner_user_id, fence)
                .await;
        }
        match result {
            Ok(_) => {}
            Err(AuthError::TokenRefreshRejected(_)) | Err(AuthError::AuthorizationRequired) => {
                return Err(McpOAuthError::AuthorizationRequired);
            }
            Err(AuthError::CredentialStoreError(message))
                if message.contains("already in progress") =>
            {
                return Err(McpOAuthError::RefreshInProgress);
            }
            Err(AuthError::CredentialStoreError(message))
                if message.contains("granted scopes beyond") =>
            {
                return Err(McpOAuthError::Disabled(
                    "the authorization server granted scopes beyond the requested read-only scope"
                        .to_string(),
                ));
            }
            Err(AuthError::TokenRefreshFailed(_)) => {
                // Transient provider/network failure: keep the stored grant.
                return Err(McpOAuthError::Upstream("token refresh failed".to_string()));
            }
            Err(other) => return Err(McpOAuthError::Upstream(describe(&other))),
        }
        let status = self
            .repository
            .mcp_oauth_credential_status(server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        tracing::info!(
            category = "mcp_oauth",
            event = "token_refreshed",
            server_id = %server.server_id,
            owner_user_id,
            "personal MCP OAuth token refreshed"
        );
        Ok(status)
    }

    /// The current access token for the owner's grant, refreshing when the
    /// stored token is expired or close to expiry.
    ///
    /// The connection epoch and generation are re-read after the refresh so a
    /// grant that was replaced or disconnected meanwhile never yields the
    /// stale token.
    pub(crate) async fn access_token(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<String, McpOAuthError> {
        let settings = self.settings(server, owner_user_id).await?;
        let loaded = self
            .repository
            .load_mcp_oauth_credential(&settings.secrets, server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        let epoch = (
            loaded.credential.connection_id,
            loaded.credential.generation,
        );
        if let Some(token) = usable_token(&loaded.credentials, loaded.credential.expires_at) {
            return Ok(token);
        }
        self.refresh(server, owner_user_id).await?;
        let reloaded = self
            .repository
            .load_mcp_oauth_credential(&settings.secrets, server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        let reloaded_epoch = (
            reloaded.credential.connection_id,
            reloaded.credential.generation,
        );
        if reloaded_epoch.0 != epoch.0 || reloaded_epoch.1 <= epoch.1 {
            // Either the grant was replaced while we refreshed, or the refresh
            // did not commit: never send an unverified token.
            return usable_token(&reloaded.credentials, reloaded.credential.expires_at)
                .ok_or(McpOAuthError::AuthorizationRequired);
        }
        usable_token(&reloaded.credentials, reloaded.credential.expires_at)
            .ok_or(McpOAuthError::AuthorizationRequired)
    }

    /// Snapshot the authorization epoch a transport is about to use.
    ///
    /// The returned context is safe to keep in memory and is the value every
    /// later send must still match.
    pub(crate) async fn send_context(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<(Uuid, String, String), McpOAuthError> {
        let settings = self.settings(server, owner_user_id).await?;
        let loaded = self
            .repository
            .load_mcp_oauth_credential(&settings.secrets, server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        Ok((
            loaded.credential.connection_id,
            loaded.credential.config_fingerprint,
            settings.server_url,
        ))
    }

    /// The access token for exactly one outbound send.
    ///
    /// The current row, owner, enabled state, configuration fingerprint and
    /// connection epoch must all still match the authorized context, both
    /// before the token is obtained and again after any awaited refresh, so a
    /// disconnect, reconnect, owner/config change or deactivation rejects this
    /// send instead of reusing a stale credential. A legitimate refresh
    /// advances the generation inside the same connection epoch and is
    /// accepted.
    pub(crate) async fn send_token(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        connection_id: Uuid,
        config_fingerprint: &str,
        url: &str,
    ) -> Result<String, McpOAuthError> {
        let server = self
            .current_send_server(
                server_id,
                owner_user_id,
                connection_id,
                config_fingerprint,
                url,
            )
            .await?;
        let token = self.access_token(&server, owner_user_id).await?;
        // The token path may have awaited a refresh; the row, owner and epoch
        // must still be the ones this transport was authorized for.
        self.current_send_server(
            server_id,
            owner_user_id,
            connection_id,
            config_fingerprint,
            url,
        )
        .await?;
        Ok(token)
    }

    async fn current_send_server(
        &self,
        server_id: Uuid,
        owner_user_id: i64,
        connection_id: Uuid,
        config_fingerprint: &str,
        url: &str,
    ) -> Result<McpServer, McpOAuthError> {
        let server = self
            .repository
            .get_mcp_server(server_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?
            .ok_or(McpOAuthError::AuthorizationRequired)?;
        if !server.enabled {
            return Err(McpOAuthError::AuthorizationRequired);
        }
        let fingerprint = authorize_mcp_oauth_server(&server, owner_user_id)
            .map_err(|_| McpOAuthError::AuthorizationRequired)?;
        if fingerprint != config_fingerprint
            || server.url.as_deref().map(str::trim) != Some(url.trim())
        {
            return Err(McpOAuthError::AuthorizationRequired);
        }
        let secrets = self.secrets()?.clone();
        let loaded = self
            .repository
            .load_mcp_oauth_credential(&secrets, server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        match loaded {
            Some(loaded) if loaded.credential.connection_id == connection_id => Ok(server),
            _ => Err(McpOAuthError::AuthorizationRequired),
        }
    }

    /// Forget the stored grant, pending states and leases for the owner.
    pub(crate) async fn disconnect(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<bool, McpOAuthError> {
        if server.scope != crate::db::MCP_OAUTH_SUPPORTED_SCOPE
            || server.owner_user_id != Some(owner_user_id)
        {
            return Err(McpOAuthError::Forbidden);
        }
        let removed = self
            .repository
            .clear_mcp_oauth_credential(server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        tracing::info!(
            category = "mcp_oauth",
            event = "disconnected",
            server_id = %server.server_id,
            owner_user_id,
            "personal MCP OAuth grant cleared"
        );
        Ok(removed)
    }

    /// Public status: `connected`, `authorizing`, `reauth_required` or
    /// `disconnected`, plus safe scopes/expiry only.
    pub(crate) async fn status(
        &self,
        server: &McpServer,
        owner_user_id: i64,
    ) -> Result<McpOAuthStatus, McpOAuthError> {
        if server.scope != crate::db::MCP_OAUTH_SUPPORTED_SCOPE
            || server.owner_user_id != Some(owner_user_id)
        {
            return Err(McpOAuthError::Forbidden);
        }
        let authorizing = match crate::db::authorize_mcp_oauth_server(server, owner_user_id) {
            Ok(fingerprint) => self
                .repository
                .has_active_mcp_oauth_state(server.server_id, owner_user_id, &fingerprint)
                .await
                .map_err(|error| McpOAuthError::Upstream(error.to_string()))?,
            Err(_) => false,
        };
        let disabled_reason = self
            .settings(server, owner_user_id)
            .await
            .err()
            .map(|error| error.message());
        let Some(secrets) = self.secrets.clone() else {
            return Ok(McpOAuthStatus {
                state: if authorizing {
                    McpOAuthState::Authorizing
                } else {
                    McpOAuthState::Disconnected
                },
                scopes: Vec::new(),
                expires_at: None,
                reason: disabled_reason,
            });
        };
        let loaded = self
            .repository
            .load_mcp_oauth_credential(&secrets, server.server_id, owner_user_id)
            .await
            .map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
        let Some(loaded) = loaded else {
            return Ok(McpOAuthStatus {
                state: if authorizing {
                    McpOAuthState::Authorizing
                } else {
                    McpOAuthState::Disconnected
                },
                scopes: Vec::new(),
                expires_at: None,
                reason: disabled_reason,
            });
        };
        let scopes = loaded.credential.scopes.clone();
        let expires_at = loaded.credential.expires_at;
        let state = if authorizing && access_token(&loaded.credentials).is_none() {
            McpOAuthState::Authorizing
        } else if usable_token(&loaded.credentials, expires_at).is_some() {
            McpOAuthState::Connected
        } else {
            McpOAuthState::ReauthRequired
        };
        Ok(McpOAuthStatus {
            state,
            scopes,
            expires_at,
            reason: disabled_reason,
        })
    }
}

fn usable_token(
    credentials: &crate::db::McpOAuthStoredCredentials,
    expires_at: Option<DateTime<Utc>>,
) -> Option<String> {
    let token = access_token(credentials)?;
    if let Some(expiry) = expires_at
        && expiry <= Utc::now() + ChronoDuration::seconds(REFRESH_SKEW_SECONDS)
    {
        return None;
    }
    Some(token)
}

/// Mirror rmcp's RFC 9207 issuer semantics on the decrypted pending state.
///
/// The stored expected issuer and `require_issuer` flag decide, and the DCR
/// metadata issuer must agree with the stored expectation when both exist. A
/// rejection here happens before the atomic consume, so the attempt survives.
fn validate_callback_issuer(
    authorization: &rmcp::transport::auth::StoredAuthorizationState,
    registration_issuer: Option<&str>,
    received_issuer: Option<&str>,
) -> Result<(), McpOAuthError> {
    if let (Some(metadata_issuer), Some(expected)) = (
        registration_issuer,
        authorization.expected_issuer.as_deref(),
    ) && metadata_issuer != expected
    {
        return Err(McpOAuthError::InvalidState);
    }
    let Some(expected_issuer) = authorization.expected_issuer.as_deref() else {
        if received_issuer.is_some() || authorization.require_issuer {
            return Err(McpOAuthError::InvalidState);
        }
        // Without issuer metadata and without an issuer-bearing callback there
        // is no stable value to bind to (rmcp's compatibility fallback).
        return Ok(());
    };
    let Some(received_issuer) = received_issuer else {
        if authorization.require_issuer {
            return Err(McpOAuthError::InvalidState);
        }
        // Older authorization servers may not send `iss`.
        return Ok(());
    };
    if received_issuer != expected_issuer {
        return Err(McpOAuthError::InvalidState);
    }
    Ok(())
}

fn validate_granted_scopes(credentials: &StoredCredentials) -> Result<(), McpOAuthError> {
    super::store::validate_stored_scopes(credentials).map_err(McpOAuthError::Disabled)
}

fn validate_redirect_base(value: &str) -> Result<(), McpOAuthError> {
    let parsed = reqwest::Url::parse(value.trim()).map_err(|_| {
        McpOAuthError::Disabled("mcp_oauth_redirect_base must be a valid HTTPS URL".to_string())
    })?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.query().is_some()
    {
        return Err(McpOAuthError::Disabled(
            "mcp_oauth_redirect_base must be a public HTTPS base URL without credentials, query or fragment"
                .to_string(),
        ));
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| {
            McpOAuthError::Disabled("mcp_oauth_redirect_base must include a host".to_string())
        })?;
    let blocked = host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(crate::mcp::transport::pinning::is_blocked_ip)
            .unwrap_or(false);
    if blocked {
        return Err(McpOAuthError::Disabled(
            "mcp_oauth_redirect_base must be a public host, not a loopback or private address"
                .to_string(),
        ));
    }
    Ok(())
}

fn choose_auth_method(metadata: &AuthorizationMetadata) -> Result<&'static str, McpOAuthError> {
    const BASIC: &str = "client_secret_basic";
    const POST: &str = "client_secret_post";
    let supported = metadata
        .additional_fields
        .get("token_endpoint_auth_methods_supported")
        .and_then(serde_json::Value::as_array);
    match supported {
        // RFC 7591 default is client_secret_basic.
        None => Ok(BASIC),
        Some(list) => {
            let has = |method: &str| list.iter().any(|value| value.as_str() == Some(method));
            if has(BASIC) {
                Ok(BASIC)
            } else if has(POST) {
                Ok(POST)
            } else {
                Err(McpOAuthError::Disabled(
                    "the authorization server supports no confidential client authentication method"
                        .to_string(),
                ))
            }
        }
    }
}

/// The method rmcp's `configure_client` will use for this metadata.
fn manager_auth_method(metadata: &AuthorizationMetadata) -> &'static str {
    let supported = metadata
        .additional_fields
        .get("token_endpoint_auth_methods_supported")
        .and_then(serde_json::Value::as_array);
    let uses_secret_post = supported
        .map(|list| {
            let has = |method: &str| list.iter().any(|value| value.as_str() == Some(method));
            has("client_secret_post") && !has("client_secret_basic")
        })
        .unwrap_or(false);
    if uses_secret_post {
        "client_secret_post"
    } else {
        "client_secret_basic"
    }
}

/// Refuse to reuse a registration whose declared method differs from the one
/// the reconstructed manager would use.
fn verify_auth_method(registration: &RegistrationSnapshot) -> Result<(), McpOAuthError> {
    if registration.client_secret.is_none() {
        return Err(McpOAuthError::Disabled(
            "the stored client registration has no client secret".to_string(),
        ));
    }
    if manager_auth_method(&registration.metadata) != registration.client_auth_method {
        return Err(McpOAuthError::Disabled(
            "the stored client authentication method no longer matches the authorization server"
                .to_string(),
        ));
    }
    Ok(())
}

async fn register_client(
    http: &PinnedOAuthHttpClient,
    registration_endpoint: &str,
    redirect_uri: &str,
    client_auth_method: &str,
) -> Result<(String, String), McpOAuthError> {
    let body = serde_json::json!({
        "client_name": "prompt-ferry",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": client_auth_method,
        "scope": MCP_OAUTH_SCOPE,
        "application_type": "web",
    });
    let payload =
        serde_json::to_vec(&body).map_err(|error| McpOAuthError::Upstream(error.to_string()))?;
    let (status, bytes) = http
        .post_json(registration_endpoint, &payload)
        .await
        .map_err(|error| McpOAuthError::Upstream(format!("client registration failed: {error}")))?;
    if !(200..300).contains(&status) {
        return Err(McpOAuthError::Disabled(format!(
            "the authorization server rejected dynamic client registration (HTTP {status})"
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
        McpOAuthError::Disabled(
            "the authorization server returned an invalid client registration response".to_string(),
        )
    })?;
    let client_id = value
        .get("client_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            McpOAuthError::Disabled(
                "dynamic client registration did not return a client identifier".to_string(),
            )
        })?;
    let client_secret = value
        .get("client_secret")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            McpOAuthError::Disabled(
                "dynamic client registration did not return a client secret".to_string(),
            )
        })?;
    Ok((client_id.to_string(), client_secret.to_string()))
}

/// The `resource` query parameter of an authorization URL, verbatim.
fn authorization_resource(authorization_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(authorization_url).ok()?;
    url.query_pairs()
        .find(|(name, _)| name == "resource")
        .map(|(_, value)| value.into_owned())
}

/// A short, non-secret description of an auth error. Provider bodies and
/// request URLs never reach the API or the log through this.
fn describe(error: &AuthError) -> String {
    let kind = match error {
        AuthError::AuthorizationRequired => "authorization required",
        AuthError::AuthorizationFailed(_) => "authorization failed",
        AuthError::TokenExchangeFailed(_) => "token exchange failed",
        AuthError::TokenRefreshFailed(_) => "token refresh failed",
        AuthError::TokenRefreshRejected(_) => "token refresh rejected",
        AuthError::CredentialStoreError(_) => "credential store error",
        AuthError::HttpError(_) => "network error",
        AuthError::OAuthError(_) => "authorization server error",
        AuthError::MetadataError(_) => "authorization metadata error",
        AuthError::PkceUnsupported => "PKCE S256 is not supported",
        AuthError::UrlError(_) => "invalid authorization URL",
        AuthError::NoAuthorizationSupport => "no authorization support detected",
        AuthError::InternalError(_) => "internal authorization error",
        AuthError::InvalidTokenType(_) => "invalid token type",
        AuthError::TokenExpired => "token expired",
        AuthError::InvalidScope(_) => "invalid scope",
        AuthError::RegistrationFailed(_) => "client registration failed",
        AuthError::InsufficientScope { .. } => "insufficient scope",
        AuthError::AuthorizationServerMismatch { .. } => "authorization server mismatch",
        AuthError::AuthorizationServerMissingIssuer { .. } => "authorization server issuer missing",
        _ => "authorization error",
    };
    kind.to_string()
}

/// Convenience wrapper used by callers that only have an `anyhow` error.
#[allow(dead_code)]
fn anyhow_upstream(error: anyhow::Error) -> McpOAuthError {
    McpOAuthError::Upstream(error.to_string())
}

#[allow(dead_code)]
fn unexpected(message: &str) -> anyhow::Error {
    anyhow!(message.to_string())
}

#[allow(dead_code)]
fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        bail!(message.to_string())
    }
}
