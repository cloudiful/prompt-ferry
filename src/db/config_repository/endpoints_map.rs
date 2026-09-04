//! Mapping helpers between the unified DTO types and the
//! PostgreSQL/SQLite-specific row models.

use anyhow::Result;

use crate::{
    config::NativeApi,
    db::{
        EndpointApiKey as PgEndpointApiKey, EndpointProvider, EndpointRegion,
        MinimaxServiceTier as PgServiceTier, ProviderEndpoint as PgProviderEndpoint,
    },
    standalone_config::{
        EndpointApiKeyConfig as ScEndpointApiKey, EndpointProvider as ScEndpointProvider,
        EndpointRegion as ScEndpointRegion, MinimaxServiceTier as ScServiceTier,
        ProviderEndpointConfig as ScProviderEndpoint,
    },
};

use super::{UnifiedEndpointApiKey, UnifiedProviderEndpoint};

pub(super) fn from_postgres(endpoint: PgProviderEndpoint) -> UnifiedProviderEndpoint {
    UnifiedProviderEndpoint {
        endpoint_id: endpoint.endpoint_id,
        scope: endpoint.scope,
        owner_user_id: endpoint.owner_user_id,
        name: endpoint.name,
        provider: endpoint.provider,
        provider_region: endpoint.provider_region,
        service_tier: endpoint.service_tier,
        base_url: endpoint.base_url,
        native_api: parse_native_api(&endpoint.native_api),
        native_api_source: endpoint.native_api_source,
        daily_max_requests: endpoint.daily_max_requests,
        monthly_max_requests: endpoint.monthly_max_requests,
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        api_keys: endpoint
            .api_keys
            .into_iter()
            .map(from_postgres_api_key)
            .collect(),
    }
}

pub(super) fn from_sqlite(endpoint: ScProviderEndpoint) -> Result<UnifiedProviderEndpoint> {
    let api_keys = endpoint
        .api_keys
        .into_iter()
        .map(from_sqlite_api_key)
        .collect::<Result<Vec<_>>>()?;
    Ok(UnifiedProviderEndpoint {
        endpoint_id: endpoint.endpoint_id,
        scope: "admin".to_string(),
        owner_user_id: None,
        name: endpoint.name,
        provider: provider_from_sqlite(endpoint.provider),
        provider_region: endpoint.provider_region.map(region_from_sqlite),
        service_tier: service_tier_from_sqlite(endpoint.service_tier),
        base_url: endpoint.base_url,
        native_api: endpoint.native_api,
        native_api_source: endpoint.native_api_source.as_str().to_string(),
        daily_max_requests: None,
        monthly_max_requests: None,
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        api_keys,
    })
}

fn from_postgres_api_key(key: PgEndpointApiKey) -> UnifiedEndpointApiKey {
    UnifiedEndpointApiKey {
        key_id: key.key_id,
        endpoint_id: key.endpoint_id,
        key_label: key.key_label,
        position: key.position,
        enabled: key.enabled,
        created_at: key.created_at,
        updated_at: key.updated_at,
    }
}

fn from_sqlite_api_key(key: ScEndpointApiKey) -> Result<UnifiedEndpointApiKey> {
    Ok(UnifiedEndpointApiKey {
        key_id: key.key_id,
        endpoint_id: key.endpoint_id,
        key_label: key.key_label,
        position: key.position,
        enabled: key.enabled,
        created_at: key.created_at,
        updated_at: key.updated_at,
    })
}

pub(super) fn parse_native_api(value: &str) -> NativeApi {
    serde_json::from_value(serde_json::Value::String(value.to_string())).unwrap_or(NativeApi::Auto)
}

pub(super) fn provider_from_sqlite(provider: ScEndpointProvider) -> EndpointProvider {
    match provider {
        ScEndpointProvider::Minimax => EndpointProvider::Minimax,
        ScEndpointProvider::Generic => EndpointProvider::Generic,
    }
}

pub(super) fn region_from_sqlite(region: ScEndpointRegion) -> EndpointRegion {
    match region {
        ScEndpointRegion::Cn => EndpointRegion::Cn,
        ScEndpointRegion::Global => EndpointRegion::Global,
    }
}

pub(super) fn service_tier_from_sqlite(tier: ScServiceTier) -> PgServiceTier {
    match tier {
        ScServiceTier::Priority => PgServiceTier::Priority,
        ScServiceTier::Standard => PgServiceTier::Standard,
    }
}

pub(crate) fn service_tier_to_sqlite(tier: PgServiceTier) -> ScServiceTier {
    match tier {
        PgServiceTier::Priority => ScServiceTier::Priority,
        PgServiceTier::Standard => ScServiceTier::Standard,
    }
}

pub(super) fn unified_to_pg(endpoint: UnifiedProviderEndpoint) -> crate::db::ProviderEndpoint {
    let created_at = endpoint.created_at;
    let updated_at = endpoint.updated_at;
    crate::db::ProviderEndpoint {
        endpoint_id: endpoint.endpoint_id,
        scope: endpoint.scope,
        owner_user_id: endpoint.owner_user_id,
        name: endpoint.name,
        provider: endpoint.provider,
        provider_region: endpoint.provider_region,
        service_tier: endpoint.service_tier,
        base_url: endpoint.base_url,
        native_api: endpoint.native_api.as_str().to_string(),
        native_api_source: endpoint.native_api_source,
        daily_max_requests: endpoint.daily_max_requests,
        monthly_max_requests: endpoint.monthly_max_requests,
        api_key: String::new(),
        key_lb_enabled: endpoint.key_lb_enabled,
        enabled: endpoint.enabled,
        mcp_enabled: endpoint.mcp_enabled,
        created_at,
        updated_at,
        api_keys: endpoint
            .api_keys
            .into_iter()
            .map(|key| crate::db::EndpointApiKey {
                key_id: key.key_id,
                endpoint_id: key.endpoint_id,
                key_label: key.key_label,
                api_key: String::new(),
                position: key.position,
                enabled: key.enabled,
                created_at: key.created_at,
                updated_at: key.updated_at,
            })
            .collect(),
    }
}
