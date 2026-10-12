//! Creating, updating and flag-setting for provider endpoints.
//!
//! Every write that touches an endpoint's key set runs on one transaction:
//! the endpoint row and its keys move together or not at all. The read that
//! builds the returned endpoint is taken on that same transaction, so the
//! caller never sees a key set from a later save than the one it triggered.

use anyhow::Result;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use crate::db::types::{
    EndpointApiKey, EndpointCreate, ProviderEndpoint, ProviderEndpointRow, normalize_service_tier,
};

use super::keys::{
    list_endpoint_api_keys_by_endpoint_id, lock_endpoint_for_update, replace_endpoint_api_keys,
};

pub async fn create_endpoint(pool: &PgPool, input: EndpointCreate) -> Result<ProviderEndpoint> {
    let mut tx = pool.begin().await?;
    let endpoint = insert_endpoint(&mut *tx, &input).await?;
    // The insert above already holds the new parent row, so the key set is
    // written under a lock that no other transaction can overtake.
    replace_endpoint_api_keys(&mut tx, endpoint.endpoint_id, &input.api_keys).await?;
    let endpoint = attach_endpoint_api_keys(&mut *tx, vec![endpoint])
        .await?
        .remove(0);
    tx.commit().await?;
    Ok(endpoint)
}

/// Insert a new endpoint with `mcp_enabled` set in the same statement so the
/// first row returned by the create flow already reflects the requested MCP
/// exposure. Generic `create_endpoint` keeps the existing default-false
/// behaviour for callers that do not care about managed MCP.
pub async fn create_endpoint_with_mcp(
    pool: &PgPool,
    input: EndpointCreate,
    mcp_enabled: bool,
) -> Result<ProviderEndpoint> {
    let mut tx = pool.begin().await?;
    let endpoint = insert_endpoint_with_mcp(&mut *tx, &input, mcp_enabled).await?;
    replace_endpoint_api_keys(&mut tx, endpoint.endpoint_id, &input.api_keys).await?;
    let endpoint = attach_endpoint_api_keys(&mut *tx, vec![endpoint])
        .await?
        .remove(0);
    tx.commit().await?;
    Ok(endpoint)
}

pub async fn update_endpoint(
    pool: &PgPool,
    endpoint_id: Uuid,
    input: EndpointCreate,
) -> Result<Option<ProviderEndpoint>> {
    let mut tx = pool.begin().await?;
    // Take the parent row's lock before the key set is read, so a second save
    // of the same endpoint waits here rather than planning its writes against
    // rows this transaction has not finished moving.
    lock_endpoint_for_update(&mut tx, endpoint_id).await?;
    let endpoint = update_endpoint_row(&mut tx, endpoint_id, &input).await?;
    let Some(endpoint) = endpoint else {
        tx.rollback().await?;
        return Ok(None);
    };
    replace_endpoint_api_keys(&mut tx, endpoint.endpoint_id, &input.api_keys).await?;
    let endpoint = attach_endpoint_api_keys(&mut *tx, vec![endpoint])
        .await?
        .remove(0);
    tx.commit().await?;
    Ok(Some(endpoint))
}

pub async fn delete_endpoint(pool: &PgPool, endpoint_id: Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/endpoints/delete_endpoint.sql", endpoint_id)
        .execute(pool)
        .await?;
    if result.rows_affected() > 0 {
        super::super::routes::cleanup_orphan_model_routes(pool).await?;
    }
    Ok(result.rows_affected() > 0)
}

pub async fn set_endpoint_mcp_enabled(
    pool: &PgPool,
    endpoint_id: Uuid,
    enabled: bool,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/endpoints/set_mcp_enabled.sql",
        endpoint_id,
        enabled
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_user_endpoint_setting(
    pool: &PgPool,
    user_id: i64,
    endpoint_id: Option<Uuid>,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/endpoints/set_user_endpoint_setting.sql",
        user_id,
        endpoint_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The endpoint fields as the store keeps them, before the row is written.
///
/// Issue #368 Phase A: an empty proxy is direct, so it becomes NULL.
/// Issue #392 Phase K: endpoint default windows already normalized by the admin
/// layer (`None`/empty means all-day -> NULL). Issue #637: the free-form tier
/// override is trimmed, and blank persists as inherit (NULL) so the
/// provider/caller default is preserved.
struct NormalizedEndpoint<'a> {
    provider_region: Option<&'a str>,
    service_tier: Option<String>,
    proxy_url: Option<String>,
    active_windows: Option<String>,
}

fn normalize_endpoint<'a>(input: &'a EndpointCreate) -> NormalizedEndpoint<'a> {
    NormalizedEndpoint {
        provider_region: input.provider_region.map(|region| region.as_str()),
        service_tier: normalize_service_tier(input.service_tier.as_deref()),
        proxy_url: input
            .proxy_url
            .clone()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        active_windows: match input.active_windows.as_deref() {
            None | Some([]) => None,
            Some(windows) => crate::db::routes::storage_value(windows),
        },
    }
}

async fn insert_endpoint<'e, E>(executor: E, input: &EndpointCreate) -> Result<ProviderEndpoint>
where
    E: Executor<'e, Database = Postgres>,
{
    let form = normalize_endpoint(input);
    Ok(sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/create_endpoint.sql",
        input.scope,
        input.owner_user_id,
        input.name,
        input.provider.as_str(),
        form.provider_region,
        form.service_tier.as_deref(),
        input.base_url,
        input.native_api.as_str(),
        input.native_api_source.as_str(),
        input.api_key,
        form.proxy_url,
        form.active_windows,
        input.key_lb_enabled,
        input.enabled,
    )
    .fetch_one(executor)
    .await
    .map(ProviderEndpoint::from)?)
}

async fn insert_endpoint_with_mcp<'e, E>(
    executor: E,
    input: &EndpointCreate,
    mcp_enabled: bool,
) -> Result<ProviderEndpoint>
where
    E: Executor<'e, Database = Postgres>,
{
    let form = normalize_endpoint(input);
    Ok(sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/create_endpoint_with_mcp.sql",
        input.scope,
        input.owner_user_id,
        input.name,
        input.provider.as_str(),
        form.provider_region,
        form.service_tier.as_deref(),
        input.base_url,
        input.native_api.as_str(),
        input.native_api_source.as_str(),
        input.api_key,
        form.proxy_url,
        form.active_windows,
        input.key_lb_enabled,
        input.enabled,
        mcp_enabled,
    )
    .fetch_one(executor)
    .await
    .map(ProviderEndpoint::from)?)
}

async fn update_endpoint_row(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    endpoint_id: Uuid,
    input: &EndpointCreate,
) -> Result<Option<ProviderEndpoint>> {
    let form = normalize_endpoint(input);
    Ok(sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/update_endpoint.sql",
        endpoint_id,
        input.scope,
        input.owner_user_id,
        input.name,
        input.provider.as_str(),
        form.provider_region,
        form.service_tier.as_deref(),
        input.base_url,
        input.native_api.as_str(),
        input.native_api_source.as_str(),
        input.api_key,
        form.proxy_url,
        form.active_windows,
        input.key_lb_enabled,
        input.enabled,
    )
    .fetch_optional(&mut **tx)
    .await?
    .map(ProviderEndpoint::from))
}

/// Read each endpoint's keys on the caller's transaction and fill them in.
///
/// An endpoint with no stored key falls back to its own legacy `api_key`, so a
/// provider endpoint that predates the key table still answers with one key.
pub(crate) async fn attach_endpoint_api_keys<'e, E>(
    executor: E,
    mut endpoints: Vec<ProviderEndpoint>,
) -> Result<Vec<ProviderEndpoint>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows_by_endpoint = list_endpoint_api_keys_by_endpoint_id(
        executor,
        &endpoints
            .iter()
            .map(|endpoint| endpoint.endpoint_id)
            .collect::<Vec<_>>(),
    )
    .await?;
    for endpoint in &mut endpoints {
        endpoint.api_keys = rows_by_endpoint
            .get(&endpoint.endpoint_id)
            .cloned()
            .unwrap_or_default();
        if endpoint.api_keys.is_empty() && !endpoint.api_key.trim().is_empty() {
            endpoint.api_keys.push(EndpointApiKey {
                key_id: Uuid::nil(),
                endpoint_id: endpoint.endpoint_id,
                key_label: endpoint.name.clone(),
                api_key: endpoint.api_key.clone(),
                position: 0,
                enabled: true,
                created_at: endpoint.created_at,
                updated_at: endpoint.updated_at,
            });
        }
    }
    Ok(endpoints)
}
