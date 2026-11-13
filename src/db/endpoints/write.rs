//! Creating, updating and flag-setting for provider endpoints.
//!
//! Every write that touches an endpoint's key set runs on one transaction:
//! the endpoint row and its keys move together or not at all. The read that
//! builds the returned endpoint is taken on that same transaction, so the
//! caller never sees a key set from a later save than the one it triggered.

use crate::db::{
    entities::postgres::provider_endpoints::{self, Column, Entity},
    seaorm::{native_error, postgres_connection},
};
use anyhow::Result;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set,
    TransactionTrait, sea_query::Expr,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{EndpointApiKey, EndpointCreate, ProviderEndpoint, normalize_service_tier};

use super::keys::{
    list_endpoint_api_keys_native, lock_endpoint_for_update, replace_endpoint_api_keys,
};
use super::load_endpoint_projection;

pub async fn create_endpoint(pool: &PgPool, input: EndpointCreate) -> Result<ProviderEndpoint> {
    create_endpoint_record(pool, input, false).await
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
    create_endpoint_record(pool, input, mcp_enabled).await
}

async fn create_endpoint_record(
    pool: &PgPool,
    input: EndpointCreate,
    mcp_enabled: bool,
) -> Result<ProviderEndpoint> {
    let db = postgres_connection(pool);
    let tx = db.begin().await.map_err(native_error)?;
    let endpoint = insert_endpoint(&tx, &input, mcp_enabled).await?;
    // The insert above already holds the new parent row, so the key set is
    // written under a lock that no other transaction can overtake.
    replace_endpoint_api_keys(&tx, endpoint.endpoint_id, &input.api_keys).await?;
    let endpoint = attach_endpoint_api_keys(&tx, vec![endpoint])
        .await?
        .remove(0);
    tx.commit().await.map_err(native_error)?;
    Ok(endpoint)
}

pub async fn update_endpoint(
    pool: &PgPool,
    endpoint_id: Uuid,
    input: EndpointCreate,
) -> Result<Option<ProviderEndpoint>> {
    let db = postgres_connection(pool);
    let tx = db.begin().await.map_err(native_error)?;
    // Take the parent row's lock before the key set is read, so a second save
    // of the same endpoint waits here rather than planning its writes against
    // rows this transaction has not finished moving.
    if !lock_endpoint_for_update(&tx, endpoint_id).await? {
        tx.rollback().await.map_err(native_error)?;
        return Ok(None);
    }
    let endpoint = update_endpoint_row(&tx, endpoint_id, &input).await?;
    let Some(endpoint) = endpoint else {
        tx.rollback().await.map_err(native_error)?;
        return Ok(None);
    };
    replace_endpoint_api_keys(&tx, endpoint.endpoint_id, &input.api_keys).await?;
    let endpoint = attach_endpoint_api_keys(&tx, vec![endpoint])
        .await?
        .remove(0);
    tx.commit().await.map_err(native_error)?;
    Ok(Some(endpoint))
}

pub async fn delete_endpoint(pool: &PgPool, endpoint_id: Uuid) -> Result<bool> {
    let result = Entity::delete_many()
        .filter(Column::EndpointId.eq(endpoint_id))
        .exec(&postgres_connection(pool))
        .await
        .map_err(native_error)?;
    if result.rows_affected > 0 {
        super::super::routes::cleanup_orphan_model_routes(pool).await?;
    }
    Ok(result.rows_affected > 0)
}

pub async fn set_endpoint_mcp_enabled(
    pool: &PgPool,
    endpoint_id: Uuid,
    enabled: bool,
) -> Result<()> {
    Entity::update_many()
        .col_expr(Column::McpEnabled, Expr::value(enabled))
        .col_expr(Column::UpdatedAt, Expr::current_timestamp())
        .filter(Column::EndpointId.eq(endpoint_id))
        .exec(&postgres_connection(pool))
        .await
        .map_err(native_error)?;
    Ok(())
}

pub async fn set_user_endpoint_setting(
    pool: &PgPool,
    user_id: i64,
    endpoint_id: Option<Uuid>,
) -> Result<()> {
    use crate::db::entities::postgres::user_endpoint_settings as settings;
    use sea_orm::sea_query::OnConflict;
    settings::Entity::insert(settings::ActiveModel {
        user_id: Set(user_id),
        endpoint_id: Set(endpoint_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(settings::Column::UserId)
            .update_column(settings::Column::EndpointId)
            .value(settings::Column::UpdatedAt, Expr::current_timestamp())
            .to_owned(),
    )
    .exec(&postgres_connection(pool))
    .await
    .map_err(native_error)?;
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

async fn insert_endpoint(
    tx: &DatabaseTransaction,
    input: &EndpointCreate,
    mcp_enabled: bool,
) -> Result<ProviderEndpoint> {
    let form = normalize_endpoint(input);
    let inserted = Entity::insert(provider_endpoints::ActiveModel {
        scope: Set(input.scope.clone()),
        owner_user_id: Set(input.owner_user_id),
        name: Set(input.name.clone()),
        provider: Set(input.provider.as_str().to_string()),
        provider_region: Set(form.provider_region.map(str::to_string)),
        service_tier: Set(form.service_tier.clone()),
        base_url: Set(input.base_url.clone()),
        native_api: Set(input.native_api.as_str().to_string()),
        native_api_source: Set(input.native_api_source.as_str().to_string()),
        api_key: Set(input.api_key.clone()),
        proxy_url: Set(form.proxy_url.clone()),
        active_windows: Set(form.active_windows.clone()),
        key_lb_enabled: Set(input.key_lb_enabled),
        enabled: Set(input.enabled),
        mcp_enabled: Set(mcp_enabled),
        ..Default::default()
    })
    .exec_with_returning_keys(tx)
    .await
    .map_err(native_error)?;
    let endpoint_id = inserted
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("inserted endpoint has no id"))?;
    load_endpoint_projection(tx, endpoint_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("inserted endpoint not found"))
}

async fn update_endpoint_row(
    tx: &DatabaseTransaction,
    endpoint_id: Uuid,
    input: &EndpointCreate,
) -> Result<Option<ProviderEndpoint>> {
    let form = normalize_endpoint(input);
    let updated = Entity::update_many()
        .col_expr(Column::Scope, Expr::value(input.scope.clone()))
        .col_expr(Column::OwnerUserId, Expr::value(input.owner_user_id))
        .col_expr(Column::Name, Expr::value(input.name.clone()))
        .col_expr(
            Column::Provider,
            Expr::value(input.provider.as_str().to_string()),
        )
        .col_expr(
            Column::ProviderRegion,
            Expr::value(form.provider_region.map(str::to_string)),
        )
        .col_expr(Column::ServiceTier, Expr::value(form.service_tier.clone()))
        .col_expr(Column::BaseUrl, Expr::value(input.base_url.clone()))
        .col_expr(
            Column::NativeApi,
            Expr::value(input.native_api.as_str().to_string()),
        )
        .col_expr(
            Column::NativeApiSource,
            Expr::value(input.native_api_source.as_str().to_string()),
        )
        .col_expr(Column::ApiKey, Expr::value(input.api_key.clone()))
        .col_expr(Column::ProxyUrl, Expr::value(form.proxy_url.clone()))
        .col_expr(
            Column::ActiveWindows,
            Expr::value(form.active_windows.clone()),
        )
        .col_expr(Column::KeyLbEnabled, Expr::value(input.key_lb_enabled))
        .col_expr(Column::Enabled, Expr::value(input.enabled))
        .col_expr(Column::UpdatedAt, Expr::current_timestamp())
        .filter(Column::EndpointId.eq(endpoint_id))
        .exec(tx)
        .await
        .map_err(native_error)?;
    if updated.rows_affected == 0 {
        return Ok(None);
    }
    load_endpoint_projection(tx, endpoint_id).await
}

/// Read each endpoint's keys on the caller's connection and fill them in.
///
/// An endpoint with no stored key falls back to its own legacy `api_key`, so a
/// provider endpoint that predates the key table still answers with one key.
pub(crate) async fn attach_endpoint_api_keys<C>(
    db: &C,
    mut endpoints: Vec<ProviderEndpoint>,
) -> Result<Vec<ProviderEndpoint>>
where
    C: ConnectionTrait,
{
    let rows_by_endpoint = list_endpoint_api_keys_native(
        db,
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
