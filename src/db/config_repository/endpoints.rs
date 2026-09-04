//! Provider endpoint CRUD for the unified configuration repository.
//!
//! The endpoint DTO is shared with the legacy admin API; mapping helpers live
//! in `endpoints_map.rs` and the PostgreSQL backing uses the SQL files in
//! `src/sql/endpoints/`. The SQLite backing uses the existing
//! `StandaloneConfigStore` so encrypted secrets are written through the
//! envelope helpers without copying the persistence layer.

use anyhow::{Context, Result};
use serde::Serialize;
use uuid::Uuid;

use super::{PostgresConfigRepository, SqliteConfigRepository, endpoints_map, endpoints_sqlite};
use crate::{
    config::NativeApi,
    db::{EndpointCreate, EndpointPage, ProviderEndpoint as PgProviderEndpoint},
};

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedProviderEndpoint {
    pub endpoint_id: Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub name: String,
    pub provider: crate::db::EndpointProvider,
    pub provider_region: Option<crate::db::EndpointRegion>,
    pub service_tier: crate::db::MinimaxServiceTier,
    pub base_url: String,
    pub native_api: NativeApi,
    pub native_api_source: String,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub key_lb_enabled: bool,
    pub enabled: bool,
    pub mcp_enabled: bool,
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
                    daily_max_requests: None,
                    monthly_max_requests: None,
                    api_key: endpoint.api_key,
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
        Ok(UnifiedEndpointPage {
            total: page.total,
            endpoints: page
                .endpoints
                .into_iter()
                .map(endpoints_map::from_postgres)
                .collect(),
            first: page.first,
            rows: page.rows,
        })
    }

    async fn get_endpoint(&self, endpoint_id: Uuid) -> Result<Option<UnifiedProviderEndpoint>> {
        Ok(crate::db::get_endpoint(&self.pool, endpoint_id)
            .await?
            .map(endpoints_map::from_postgres))
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
        let unified = endpoints
            .into_iter()
            .map(endpoints_map::from_sqlite)
            .collect::<Result<Vec<_>>>()?;
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
        endpoint.map(endpoints_map::from_sqlite).transpose()
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
            service_tier: crate::db::MinimaxServiceTier::Standard,
            base_url: "https://example.test".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: "manual".to_string(),
            daily_max_requests: None,
            monthly_max_requests: None,
            key_lb_enabled: false,
            enabled: true,
            mcp_enabled: false,
            created_at: now,
            updated_at: now,
            api_keys: vec![],
        };
        let pg: PgProviderEndpoint = unified.into();
        assert_eq!(pg.endpoint_id, endpoint_id);
        assert_eq!(pg.base_url, "https://example.test");
    }
}
