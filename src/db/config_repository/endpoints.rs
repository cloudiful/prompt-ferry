//! Provider endpoint CRUD for the unified configuration repository.
//!
//! The endpoint DTO is shared with the legacy admin API; mapping helpers live
//! in `endpoints_map.rs` and the PostgreSQL backing uses the SQL files in
//! `src/sql/endpoints/`. The SQLite backing uses the existing
//! `StandaloneConfigStore` so encrypted secrets are written through the
//! envelope helpers without copying the persistence layer.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

use super::{PostgresConfigRepository, SqliteConfigRepository, endpoints_map, endpoints_sqlite};
use crate::{
    config::NativeApi,
    db::{
        EndpointCreate, EndpointOAuthToken, EndpointOAuthTokenSet, EndpointPage, EndpointPlan,
        ProviderEndpoint as PgProviderEndpoint,
    },
};

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedProviderEndpoint {
    pub endpoint_id: Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: crate::db::EndpointProvider,
    pub provider_region: Option<crate::db::EndpointRegion>,
    /// Issue #599 R2a: upstream plan axis, derived server-side from the
    /// stored OAuth token (`EndpointPlan::resolve`). Never trusted blindly
    /// from the client.
    pub plan: EndpointPlan,
    pub service_tier: crate::db::MinimaxServiceTier,
    pub base_url: String,
    pub native_api: NativeApi,
    pub native_api_source: String,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
    // Issue #368 Phase C (P2): response-side saved-proxy indicator.
    // `true` when a proxy URL is stored; the secret itself is never echoed.
    pub has_proxy_url: bool,
    // Issue #599 R2a: response-side saved-OAuth-token indicator. `true` when
    // a ChatGPT OAuth token is stored; the secrets themselves are never
    // echoed, mirroring `has_proxy_url`.
    pub has_oauth_token: bool,
    // Issue #392 Phase K: endpoint default windows (empty means all-day).
    pub active_windows: Vec<crate::db::ActiveWindow>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub api_keys: Vec<UnifiedEndpointApiKey>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedEndpointApiKey {
    pub key_id: Uuid,
    pub endpoint_id: Uuid,
    pub key_label: String,
    pub position: i32,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedEndpointPage {
    pub total: i64,
    pub endpoints: Vec<UnifiedProviderEndpoint>,
    pub first: i64,
    pub rows: i64,
}

impl From<UnifiedProviderEndpoint> for PgProviderEndpoint {
    fn from(value: UnifiedProviderEndpoint) -> Self {
        endpoints_map::unified_to_pg(value)
    }
}

impl From<UnifiedEndpointPage> for crate::worker_admin_types::EndpointPageResponse {
    fn from(value: UnifiedEndpointPage) -> Self {
        Self {
            total: value.total,
            endpoints: value
                .endpoints
                .into_iter()
                .map(endpoints_map::unified_to_pg)
                .collect(),
            first: value.first,
            rows: value.rows,
        }
    }
}

impl UnifiedProviderEndpoint {
    /// Render the unified endpoint into the legacy `ProviderEndpoint` shape
    /// that the existing admin API returns to callers.
    pub fn into_pg(self) -> PgProviderEndpoint {
        endpoints_map::unified_to_pg(self)
    }
}

/// Issue #599 R2a: stamp the derived plan + token presence onto a unified
/// endpoint after mapping. `token_ids` holds every endpoint with a stored
/// (non-cleared) OAuth token.
fn stamp_oauth_presence(endpoint: &mut UnifiedProviderEndpoint, token_ids: &HashSet<Uuid>) {
    endpoint.has_oauth_token = token_ids.contains(&endpoint.endpoint_id);
    endpoint.plan = EndpointPlan::resolve(endpoint.provider, endpoint.has_oauth_token);
}

impl super::ConfigRepository {
    pub async fn list_endpoints_page(&self, first: i64, rows: i64) -> Result<UnifiedEndpointPage> {
        match self {
            Self::Postgres(repo) => repo.list_endpoints_page(first, rows).await,
            Self::Sqlite(repo) => repo.list_endpoints_page(first, rows).await,
        }
    }

    pub async fn get_endpoint(&self, endpoint_id: Uuid) -> Result<Option<UnifiedProviderEndpoint>> {
        match self {
            Self::Postgres(repo) => repo.get_endpoint(endpoint_id).await,
            Self::Sqlite(repo) => repo.get_endpoint(endpoint_id).await,
        }
    }

    /// Load an endpoint with its decrypted API keys for an internal provider
    /// integration. This is intentionally crate-private; admin responses use
    /// the redacted unified endpoint shape above.
    pub(crate) async fn get_endpoint_for_mcp(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<PgProviderEndpoint>> {
        match self {
            Self::Postgres(repo) => crate::db::get_endpoint(repo.pool(), endpoint_id).await,
            Self::Sqlite(repo) => {
                let Some(endpoint) = repo
                    .store()
                    .get_endpoint(repo.manager(), endpoint_id)
                    .await
                    .map_err(|err| anyhow::anyhow!("{err}"))?
                else {
                    return Ok(None);
                };
                Ok(Some(crate::db::ProviderEndpoint {
                    endpoint_id: endpoint.endpoint_id,
                    scope: "admin".to_string(),
                    owner_user_id: None,
                    name: endpoint.name,
                    provider: match endpoint.provider {
                        crate::standalone_config::EndpointProvider::Minimax => {
                            crate::db::EndpointProvider::Minimax
                        }
                        crate::standalone_config::EndpointProvider::CommandCode => {
                            crate::db::EndpointProvider::CommandCode
                        }
                        crate::standalone_config::EndpointProvider::OpencodeGo => {
                            crate::db::EndpointProvider::OpencodeGo
                        }
                        crate::standalone_config::EndpointProvider::OpenRouter => {
                            crate::db::EndpointProvider::OpenRouter
                        }
                        crate::standalone_config::EndpointProvider::Glm => {
                            crate::db::EndpointProvider::Glm
                        }
                        crate::standalone_config::EndpointProvider::DeepSeek => {
                            crate::db::EndpointProvider::DeepSeek
                        }
                        crate::standalone_config::EndpointProvider::OpenAi => {
                            crate::db::EndpointProvider::OpenAi
                        }
                        crate::standalone_config::EndpointProvider::Generic => {
                            crate::db::EndpointProvider::Generic
                        }
                    },
                    provider_region: endpoint.provider_region.map(|region| match region {
                        crate::standalone_config::EndpointRegion::Cn => {
                            crate::db::EndpointRegion::Cn
                        }
                        crate::standalone_config::EndpointRegion::Global => {
                            crate::db::EndpointRegion::Global
                        }
                    }),
                    service_tier: endpoints_map::service_tier_from_sqlite(endpoint.service_tier),
                    base_url: endpoint.base_url,
                    native_api: endpoint.native_api.as_str().to_string(),
                    native_api_source: endpoint.native_api_source.as_str().to_string(),
                    api_key: endpoint.api_key,
                    proxy_url: endpoint.proxy_url.clone(),
                    // Issue #368 Phase C (P2): internal MCP shape also
                    // carries the saved indicator for consistency.
                    has_proxy_url: endpoint
                        .proxy_url
                        .as_deref()
                        .is_some_and(|raw| !raw.trim().is_empty()),
                    // Issue #599 R2a: the internal MCP shape stays on the
                    // platform plan; subscription routing arrives in R2c.
                    plan: crate::db::EndpointPlan::default(),
                    has_oauth_token: false,
                    // Issue #392 Phase K: 0021 plaintext schedule for display.
                    active_windows: crate::db::parse_stored_windows(
                        endpoint.active_windows.as_deref(),
                    )
                    .unwrap_or_default(),
                    key_lb_enabled: endpoint.key_lb_enabled,
                    enabled: endpoint.enabled,
                    mcp_enabled: endpoint.mcp_enabled,
                    created_at: endpoint.created_at,
                    updated_at: endpoint.updated_at,
                    api_keys: endpoint
                        .api_keys
                        .into_iter()
                        .map(|key| crate::db::EndpointApiKey {
                            key_id: key.key_id,
                            endpoint_id: key.endpoint_id,
                            key_label: key.key_label,
                            api_key: key.api_key,
                            position: key.position,
                            enabled: key.enabled,
                            created_at: key.created_at,
                            updated_at: key.updated_at,
                        })
                        .collect(),
                }))
            }
        }
    }

    pub async fn create_endpoint(
        &self,
        endpoint_id: Uuid,
        input: EndpointCreate,
        mcp_enabled: bool,
    ) -> Result<UnifiedProviderEndpoint> {
        match self {
            Self::Postgres(repo) => repo.create_endpoint(input, mcp_enabled).await,
            Self::Sqlite(repo) => repo.create_endpoint(endpoint_id, input, mcp_enabled).await,
        }
    }

    pub async fn update_endpoint(
        &self,
        endpoint_id: Uuid,
        input: EndpointCreate,
    ) -> Result<Option<UnifiedProviderEndpoint>> {
        match self {
            Self::Postgres(repo) => repo.update_endpoint(endpoint_id, input).await,
            Self::Sqlite(repo) => repo.update_endpoint(endpoint_id, input).await,
        }
    }

    pub async fn set_endpoint_mcp_enabled(&self, endpoint_id: Uuid, enabled: bool) -> Result<()> {
        match self {
            Self::Postgres(repo) => {
                crate::db::set_endpoint_mcp_enabled(repo.pool(), endpoint_id, enabled).await
            }
            Self::Sqlite(repo) => repo.set_endpoint_mcp_enabled(endpoint_id, enabled).await,
        }
    }

    pub async fn delete_endpoint(&self, endpoint_id: Uuid) -> Result<bool> {
        match self {
            Self::Postgres(repo) => crate::db::delete_endpoint(repo.pool(), endpoint_id).await,
            Self::Sqlite(repo) => repo.delete_endpoint(endpoint_id).await,
        }
    }

    pub async fn first_endpoint_api_key(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        match self {
            Self::Postgres(repo) => repo.first_endpoint_api_key(endpoint_id).await,
            Self::Sqlite(repo) => repo.first_endpoint_api_key(endpoint_id).await,
        }
    }

    /// Issue #368 Phase A: decrypted outbound proxy default for an endpoint.
    /// `None` means direct. PG reads the plaintext column; SQLite decrypts
    /// the 0018 envelope. Admin listings never expose this value.
    pub async fn endpoint_proxy_url(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        match self {
            Self::Postgres(repo) => repo.endpoint_proxy_url(endpoint_id).await,
            Self::Sqlite(repo) => repo.endpoint_proxy_url(endpoint_id).await,
        }
    }

    /// Issue #599 R2a: store (`Some`) or clear (`None`, NULL convention) the
    /// ChatGPT OAuth token for an endpoint. The endpoint must exist either
    /// way. The OpenAI-only gate guards storing a credential, so a non-OpenAI
    /// endpoint can never acquire one; clearing deliberately bypasses that
    /// gate, because a provider or plan switch moves the endpoint first and
    /// only then drops the orphaned token (issue #599 R2a P1). Secrets stay
    /// server-side; admin responses only ever see `has_oauth_token`.
    pub async fn set_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
        token: Option<EndpointOAuthTokenSet>,
    ) -> Result<()> {
        let endpoint = self
            .get_endpoint(endpoint_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("endpoint {endpoint_id} not found"))?;
        if token.is_some() && !endpoint.provider.supports_chatgpt_subscription_plan() {
            anyhow::bail!(
                "chatgpt oauth token requires an openai endpoint (found {:?})",
                endpoint.provider
            );
        }
        match self {
            Self::Postgres(repo) => repo.set_endpoint_oauth_token(endpoint_id, token).await,
            Self::Sqlite(repo) => repo.set_endpoint_oauth_token(endpoint_id, token).await,
        }
    }

    /// Issue #599 R2a: drop the stored OAuth token (plan-switch hygiene).
    /// Works on any provider so a switch away from OpenAI still removes the
    /// orphaned credential; never fails for a missing token and leaves the
    /// endpoint itself untouched.
    pub async fn clear_endpoint_oauth_token(&self, endpoint_id: Uuid) -> Result<()> {
        self.set_endpoint_oauth_token(endpoint_id, None).await
    }

    /// Issue #599 R2a: load the decrypted OAuth token for internal use (token
    /// refresh in R2b). Crate-private on purpose: the return type carries
    /// secrets, so no admin response type may contain it. `None` means absent
    /// or cleared.
    // Issue #599 R2b will consume this dispatcher for token refresh;
    // single-fetch presence reuses the backend secret reads above.
    #[allow(dead_code)]
    pub(crate) async fn get_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<EndpointOAuthToken>> {
        match self {
            Self::Postgres(repo) => repo.get_endpoint_oauth_token(endpoint_id).await,
            Self::Sqlite(repo) => repo.get_endpoint_oauth_token(endpoint_id).await,
        }
    }

    /// Issue #599 R2a: IDs of endpoints with a stored (non-cleared) OAuth
    /// token. Drives the derived plan and the admin `has_oauth_token`
    /// indicator without ever touching the secrets themselves.
    pub async fn list_endpoint_oauth_token_ids(&self) -> Result<Vec<Uuid>> {
        match self {
            Self::Postgres(repo) => repo.list_endpoint_oauth_token_ids().await,
            Self::Sqlite(repo) => repo.list_endpoint_oauth_token_ids().await,
        }
    }

    pub async fn get_user_endpoint_setting(&self, user_id: i64) -> Result<Option<Uuid>> {
        match self {
            Self::Postgres(repo) => {
                crate::db::get_user_endpoint_setting(repo.pool(), user_id).await
            }
            Self::Sqlite(_) => Ok(None),
        }
    }

    pub async fn set_user_endpoint_setting(
        &self,
        user_id: i64,
        endpoint_id: Option<Uuid>,
    ) -> Result<()> {
        match self {
            Self::Postgres(repo) => {
                crate::db::set_user_endpoint_setting(repo.pool(), user_id, endpoint_id).await
            }
            Self::Sqlite(_) => Ok(()),
        }
    }

    /// Look up existing API-key rows (including plaintext secrets) so a PATCH
    /// handler can carry forward unchanged secrets when the request omits
    /// them. Only the first API key value is ever exposed by this helper.
    pub async fn endpoint_api_keys_for_update(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Vec<crate::db::EndpointApiKey>> {
        match self {
            Self::Postgres(repo) => repo.endpoint_api_keys_for_update(endpoint_id).await,
            Self::Sqlite(repo) => repo.endpoint_api_keys_for_update(endpoint_id).await,
        }
    }
}

impl PostgresConfigRepository {
    async fn list_endpoints_page(&self, first: i64, rows: i64) -> Result<UnifiedEndpointPage> {
        let page: EndpointPage = crate::db::list_endpoints_page(&self.pool, first, rows).await?;
        // Issue #599 R2a: one presence query stamps the whole page (no N+1);
        // the secrets themselves never leave the token table.
        let token_ids = self
            .list_endpoint_oauth_token_ids()
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        Ok(UnifiedEndpointPage {
            total: page.total,
            endpoints: page
                .endpoints
                .into_iter()
                .map(|endpoint| {
                    let mut unified = endpoints_map::from_postgres(endpoint);
                    stamp_oauth_presence(&mut unified, &token_ids);
                    unified
                })
                .collect(),
            first: page.first,
            rows: page.rows,
        })
    }

    async fn get_endpoint(&self, endpoint_id: Uuid) -> Result<Option<UnifiedProviderEndpoint>> {
        let endpoint = crate::db::get_endpoint(&self.pool, endpoint_id)
            .await?
            .map(endpoints_map::from_postgres);
        let Some(mut unified) = endpoint else {
            return Ok(None);
        };
        // Issue #599 R2a: presence reuses the secret read; the row is dropped
        // without logging, keeping single-endpoint fetches at one extra
        // indexed query.
        let present = self.get_endpoint_oauth_token(endpoint_id).await?.is_some();
        unified.has_oauth_token = present;
        unified.plan = EndpointPlan::resolve(unified.provider, present);
        Ok(Some(unified))
    }

    async fn create_endpoint(
        &self,
        input: EndpointCreate,
        mcp_enabled: bool,
    ) -> Result<UnifiedProviderEndpoint> {
        let endpoint = crate::db::create_endpoint_with_mcp(&self.pool, input, mcp_enabled)
            .await
            .context("failed to create endpoint")?;
        Ok(endpoints_map::from_postgres(endpoint))
    }

    async fn update_endpoint(
        &self,
        endpoint_id: Uuid,
        input: EndpointCreate,
    ) -> Result<Option<UnifiedProviderEndpoint>> {
        Ok(crate::db::update_endpoint(&self.pool, endpoint_id, input)
            .await?
            .map(endpoints_map::from_postgres))
    }

    async fn first_endpoint_api_key(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        let endpoint = crate::db::get_endpoint(&self.pool, endpoint_id).await?;
        Ok(endpoint.map(|e| e.api_key))
    }

    /// Issue #599 R2a: upsert (`Some`) or clear (`None`, NULL convention) the
    /// OAuth token row. The caller (`ConfigRepository::set_...`) enforces the
    /// OpenAI-only gate, so this stays a plain secret write.
    async fn set_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
        token: Option<EndpointOAuthTokenSet>,
    ) -> Result<()> {
        let (access_token, refresh_token, expires_at) = match token {
            Some(token) => (
                Some(token.access_token),
                Some(token.refresh_token),
                token.expires_at,
            ),
            None => (None, None, None),
        };
        sqlx::query_file!(
            "src/sql/endpoints/set_endpoint_oauth_token.sql",
            endpoint_id,
            access_token,
            refresh_token,
            expires_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<EndpointOAuthToken>> {
        let row = sqlx::query_file!(
            "src/sql/endpoints/get_endpoint_oauth_token.sql",
            endpoint_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        // `query_file!` yields a generated record struct, so columns are read
        // as fields; nullable columns stay `Option`, matching the clear
        // convention below.
        let access_token = row.access_token;
        let refresh_token = row.refresh_token;
        let expires_at = row.expires_at;
        match (access_token, refresh_token) {
            (Some(access_token), Some(refresh_token)) => Ok(Some(EndpointOAuthToken {
                endpoint_id,
                access_token,
                refresh_token,
                expires_at,
            })),
            // Cleared (NULL convention) or never stored.
            (None, None) => Ok(None),
            // Refresh without access cannot be used; fail closed like SQLite.
            (None, Some(_)) => {
                anyhow::bail!("endpoint oauth token is missing its access token")
            }
            // Stale access without refresh reads as absent (clear marker).
            (Some(_), None) => Ok(None),
        }
    }

    async fn list_endpoint_oauth_token_ids(&self) -> Result<Vec<Uuid>> {
        let rows = sqlx::query_file!("src/sql/endpoints/list_endpoint_oauth_token_ids.sql")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|row| row.endpoint_id).collect())
    }

    async fn endpoint_proxy_url(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        let endpoint = crate::db::get_endpoint(&self.pool, endpoint_id).await?;
        Ok(endpoint.and_then(|e| e.proxy_url))
    }

    async fn endpoint_api_keys_for_update(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Vec<crate::db::EndpointApiKey>> {
        let rows =
            crate::db::endpoints::list_endpoint_api_keys_by_endpoint_id(&self.pool, &[endpoint_id])
                .await?;
        Ok(rows.get(&endpoint_id).cloned().unwrap_or_default())
    }
}

impl SqliteConfigRepository {
    async fn list_endpoints_page(&self, first: i64, rows: i64) -> Result<UnifiedEndpointPage> {
        let (total, endpoints) = self
            .store
            .list_endpoints_page(&self.manager, first, rows)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let mut unified = endpoints
            .into_iter()
            .map(endpoints_map::from_sqlite)
            .collect::<Result<Vec<_>>>()?;
        // Issue #599 R2a: one presence query stamps the whole page (no N+1).
        let token_ids = self
            .list_endpoint_oauth_token_ids()
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        for endpoint in &mut unified {
            stamp_oauth_presence(endpoint, &token_ids);
        }
        Ok(UnifiedEndpointPage {
            total,
            endpoints: unified,
            first,
            rows,
        })
    }

    async fn get_endpoint(&self, endpoint_id: Uuid) -> Result<Option<UnifiedProviderEndpoint>> {
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(endpoint) = endpoint else {
            return Ok(None);
        };
        let mut unified = endpoints_map::from_sqlite(endpoint)?;
        let present = self.get_endpoint_oauth_token(endpoint_id).await?.is_some();
        unified.has_oauth_token = present;
        unified.plan = EndpointPlan::resolve(unified.provider, present);
        Ok(Some(unified))
    }

    async fn create_endpoint(
        &self,
        endpoint_id: Uuid,
        input: EndpointCreate,
        _mcp_enabled: bool,
    ) -> Result<UnifiedProviderEndpoint> {
        let config = endpoints_sqlite::sqlite_endpoint_from_create(
            endpoint_id,
            input,
            _mcp_enabled,
            endpoints_sqlite::EndpointTimestamps::default(),
        )?;
        self.store
            .save_endpoint(&self.manager, &config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
            .ok_or_else(|| anyhow::anyhow!("endpoint not found after insert"))?;
        endpoints_map::from_sqlite(endpoint)
    }

    async fn update_endpoint(
        &self,
        endpoint_id: Uuid,
        input: EndpointCreate,
    ) -> Result<Option<UnifiedProviderEndpoint>> {
        let existing = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(existing_endpoint) = existing else {
            return Ok(None);
        };
        // Build a key lookup keyed by `key_id` so we can carry forward the
        // original `created_at`/`updated_at` for keys that survived the PATCH
        // without their secret being replaced.
        let api_key_timestamps = existing_endpoint
            .api_keys
            .iter()
            .map(|key| endpoints_sqlite::ApiKeyTimestamp {
                key_id: key.key_id,
                created_at: key.created_at,
                updated_at: key.updated_at,
            })
            .collect();
        let timestamps = endpoints_sqlite::EndpointTimestamps {
            endpoint_created_at: Some(existing_endpoint.created_at),
            endpoint_updated_at: Some(existing_endpoint.updated_at),
            api_key_timestamps,
        };
        let config =
            endpoints_sqlite::sqlite_endpoint_from_create(endpoint_id, input, false, timestamps)?;
        self.store
            .save_endpoint(&self.manager, &config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        endpoint.map(endpoints_map::from_sqlite).transpose()
    }

    async fn set_endpoint_mcp_enabled(&self, endpoint_id: Uuid, enabled: bool) -> Result<()> {
        self.store
            .set_endpoint_mcp_enabled(endpoint_id, enabled)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }

    async fn delete_endpoint(&self, endpoint_id: Uuid) -> Result<bool> {
        self.store
            .delete_endpoint(endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }

    async fn first_endpoint_api_key(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(endpoint.map(|e| e.api_key))
    }

    async fn set_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
        token: Option<EndpointOAuthTokenSet>,
    ) -> Result<()> {
        self.store
            .set_endpoint_oauth_token(&self.manager, endpoint_id, token)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }

    async fn get_endpoint_oauth_token(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Option<EndpointOAuthToken>> {
        self.store
            .get_endpoint_oauth_token(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }

    async fn list_endpoint_oauth_token_ids(&self) -> Result<Vec<Uuid>> {
        self.store
            .list_endpoint_oauth_token_ids()
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }

    async fn endpoint_proxy_url(&self, endpoint_id: Uuid) -> Result<Option<String>> {
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(endpoint.and_then(|e| e.proxy_url))
    }

    async fn endpoint_api_keys_for_update(
        &self,
        endpoint_id: Uuid,
    ) -> Result<Vec<crate::db::EndpointApiKey>> {
        let endpoint = self
            .store
            .get_endpoint(&self.manager, endpoint_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(endpoint) = endpoint else {
            return Ok(Vec::new());
        };
        Ok(endpoint
            .api_keys
            .into_iter()
            .map(|key| crate::db::EndpointApiKey {
                key_id: key.key_id,
                endpoint_id: key.endpoint_id,
                key_label: key.key_label,
                api_key: key.api_key,
                position: key.position,
                enabled: key.enabled,
                created_at: key.created_at,
                updated_at: key.updated_at,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_endpoint_round_trips_via_mapper() {
        let endpoint_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let unified = UnifiedProviderEndpoint {
            endpoint_id,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "primary".to_string(),
            provider: crate::db::EndpointProvider::Generic,
            provider_region: None,
            plan: crate::db::EndpointPlan::PlatformApiKey,
            service_tier: crate::db::MinimaxServiceTier::Standard,
            base_url: "https://example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: "manual".to_string(),
            key_lb_enabled: false,
            enabled: true,
            mcp_enabled: false,
            has_proxy_url: false,
            has_oauth_token: false,
            active_windows: vec![],
            created_at: now,
            updated_at: now,
            api_keys: vec![],
        };
        let pg: PgProviderEndpoint = unified.into();
        assert_eq!(pg.endpoint_id, endpoint_id);
        assert_eq!(pg.base_url, "https://example.test");
    }
}
