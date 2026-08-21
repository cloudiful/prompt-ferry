//! Helpers for converting the unified endpoint DTO request types into the
//! SQLite row model used by `StandaloneConfigStore`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    db::{EndpointApiKeyCreate, EndpointCreate, EndpointRegion},
    standalone_config::{
        EndpointApiKeyConfig as ScEndpointApiKey, EndpointProvider as ScEndpointProvider,
        EndpointRegion as ScEndpointRegion, ProviderEndpointConfig as ScProviderEndpoint,
    },
};

/// Optional carry-forward of the persisted timestamps when a PATCH keeps an
/// existing endpoint or endpoint API key. `None` values mean the mapper
/// should assign fresh `Utc::now()` values (i.e. a brand-new row).
#[derive(Debug, Default, Clone)]
pub(super) struct EndpointTimestamps {
    pub endpoint_created_at: Option<DateTime<Utc>>,
    pub endpoint_updated_at: Option<DateTime<Utc>>,
    pub api_key_timestamps: Vec<ApiKeyTimestamp>,
}

#[derive(Debug, Clone)]
pub(super) struct ApiKeyTimestamp {
    pub key_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub(super) fn sqlite_endpoint_from_create(
    endpoint_id: Uuid,
    input: EndpointCreate,
    timestamps: EndpointTimestamps,
) -> Result<ScProviderEndpoint> {
    if input.api_key.trim().is_empty() && input.api_keys.is_empty() {
        anyhow::bail!("endpoint api key is required");
    }
    let api_key = if !input.api_key.trim().is_empty() {
        input.api_key.clone()
    } else {
        input
            .api_keys
            .first()
            .map(|key| key.api_key.clone())
            .unwrap_or_default()
    };
    let now = Utc::now();
    let api_key_lookup: std::collections::HashMap<Uuid, &ApiKeyTimestamp> = timestamps
        .api_key_timestamps
        .iter()
        .map(|ts| (ts.key_id, ts))
        .collect();
    let api_keys = input
        .api_keys
        .into_iter()
        .map(
            |EndpointApiKeyCreate {
                 key_id,
                 key_label,
                 api_key,
                 position,
                 enabled,
             }| {
                let resolved_id = key_id.unwrap_or_else(Uuid::new_v4);
                let (created_at, updated_at) = api_key_lookup
                    .get(&resolved_id)
                    .map(|ts| (ts.created_at, ts.updated_at))
                    .unwrap_or((now, now));
                ScEndpointApiKey {
                    key_id: resolved_id,
                    endpoint_id,
                    key_label,
                    api_key,
                    position,
                    enabled,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect::<Vec<_>>();

    let provider = match input.provider {
        crate::db::EndpointProvider::Minimax => ScEndpointProvider::Minimax,
        crate::db::EndpointProvider::Generic => ScEndpointProvider::Generic,
    };
    let provider_region = match input.provider_region {
        Some(EndpointRegion::Cn) => Some(ScEndpointRegion::Cn),
        Some(EndpointRegion::Global) => Some(ScEndpointRegion::Global),
        None => None,
    };
    Ok(ScProviderEndpoint {
        endpoint_id,
        name: input.name,
        provider,
        provider_region,
        base_url: input.base_url,
        native_api: input.native_api,
        native_api_source: input.native_api_source,
        key_lb_enabled: input.key_lb_enabled,
        enabled: input.enabled,
        mcp_enabled: false,
        created_at: timestamps.endpoint_created_at.unwrap_or(now),
        updated_at: timestamps.endpoint_updated_at.unwrap_or(now),
        api_key,
        api_keys,
    })
}
