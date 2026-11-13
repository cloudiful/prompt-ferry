use crate::db::entities::postgres::provider_endpoints::{self, Column, Entity};
use crate::db::seaorm::postgres_connection;
use anyhow::Result;
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use sqlx::PgPool;
use std::collections::HashSet;

use crate::{
    config::NativeApi,
    db::types::{
        EndpointPage, ProviderEndpoint, ProviderEndpointRow, RouteConfig, normalize_service_tier,
    },
};

mod keys;
mod keys_diff;
mod write;

#[cfg(test)]
mod keys_diff_tests;

// Read paths outside this module hydrate endpoint key sets too.
pub(crate) use keys::list_endpoint_api_keys_by_endpoint_id;
use write::attach_endpoint_api_keys;

pub use write::{
    create_endpoint, create_endpoint_with_mcp, delete_endpoint, set_endpoint_mcp_enabled,
    set_user_endpoint_setting, update_endpoint,
};

#[derive(FromQueryResult)]
struct EndpointProjection {
    endpoint_id: uuid::Uuid,
    scope: String,
    owner_user_id: Option<i64>,
    name: String,
    provider: String,
    provider_region: Option<String>,
    service_tier: Option<String>,
    base_url: String,
    native_api: String,
    native_api_source: String,
    api_key: String,
    proxy_url: Option<String>,
    active_windows: Option<String>,
    key_lb_enabled: bool,
    enabled: bool,
    mcp_enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<EndpointProjection> for ProviderEndpoint {
    fn from(row: EndpointProjection) -> Self {
        ProviderEndpointRow {
            endpoint_id: row.endpoint_id,
            scope: row.scope,
            owner_user_id: row.owner_user_id,
            name: row.name,
            provider: row.provider,
            provider_region: row.provider_region,
            service_tier: row.service_tier,
            base_url: row.base_url,
            native_api: row.native_api,
            native_api_source: row.native_api_source,
            api_key: row.api_key,
            proxy_url: row.proxy_url,
            active_windows: row.active_windows,
            key_lb_enabled: row.key_lb_enabled,
            enabled: row.enabled,
            mcp_enabled: row.mcp_enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
        .into()
    }
}

fn endpoint_projection() -> sea_orm::Select<provider_endpoints::Entity> {
    Entity::find().select_only().columns([
        Column::EndpointId,
        Column::Scope,
        Column::OwnerUserId,
        Column::Name,
        Column::Provider,
        Column::ProviderRegion,
        Column::ServiceTier,
        Column::BaseUrl,
        Column::NativeApi,
        Column::NativeApiSource,
        Column::ApiKey,
        Column::ProxyUrl,
        Column::ActiveWindows,
        Column::KeyLbEnabled,
        Column::Enabled,
        Column::McpEnabled,
        Column::CreatedAt,
        Column::UpdatedAt,
    ])
}

/// Load one endpoint through the ordinary non-secret projection.
///
/// The caller's connection decides transactional visibility: the write paths
/// pass their own transaction so the row they just wrote is read without an
/// autocommit race, and the projection never selects `admin_api_key`.
pub(super) async fn load_endpoint_projection<C>(
    db: &C,
    endpoint_id: uuid::Uuid,
) -> Result<Option<ProviderEndpoint>>
where
    C: ConnectionTrait,
{
    Ok(endpoint_projection()
        .filter(Column::EndpointId.eq(endpoint_id))
        .into_model::<EndpointProjection>()
        .one(db)
        .await?
        .map(ProviderEndpoint::from))
}

pub async fn list_endpoints(pool: &PgPool) -> Result<Vec<ProviderEndpoint>> {
    let db = postgres_connection(pool);
    let endpoints = endpoint_projection()
        .order_by_desc(Column::UpdatedAt)
        .into_model::<EndpointProjection>()
        .all(&db)
        .await?
        .into_iter()
        .map(ProviderEndpoint::from)
        .collect();
    attach_endpoint_api_keys(&db, endpoints).await
}

pub async fn list_visible_endpoints(pool: &PgPool, user_id: i64) -> Result<Vec<RouteConfig>> {
    let rows = sqlx::query_file!("src/sql/endpoints/list_visible_endpoints.sql", user_id)
        .fetch_all(pool)
        .await?;

    let routes = rows
        .into_iter()
        .map(|row| RouteConfig {
            embedding_dimensions_override: None,
            route_id: row.route_id,
            user_id: row.user_id,
            model_route_rule_id: None,
            base_url: row.base_url,
            api_key: row.api_key,
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: row.key_lb_enabled,
            native_api: parse_native_api(&row.native_api),
            upstream_model: None,
            route_selection_reason: crate::db::RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::from_str_or_default(&row.provider),
            service_tier: normalize_service_tier(row.service_tier.as_deref()),
            proxy_url: row.proxy_url,
            // Issue #392 Phase K: direct endpoint routes never normalize.
            dev_system_normalize: false,
            // Issue #566: direct endpoint routes never enable the adaption.
            thinking_downgrade_enabled: false,
            // Issue #464: direct routes follow the caller.
            thinking_effort_override: None,
            compact_mode: crate::db::CompactMode::Passthrough,
            // Issue #1004 P1: stamped from the token-id existence query.
            has_oauth_token: false,
        })
        .collect::<Vec<_>>();
    attach_route_config_credentials(pool, routes).await
}

pub async fn list_endpoints_page(pool: &PgPool, first: i64, rows: i64) -> Result<EndpointPage> {
    let db = postgres_connection(pool);
    let total = Entity::find().count(&db).await? as i64;
    let endpoints = endpoint_projection()
        .order_by_desc(Column::UpdatedAt)
        .order_by_desc(Column::EndpointId)
        .offset(first.max(0) as u64)
        .limit(rows.clamp(1, 200) as u64)
        .into_model::<EndpointProjection>()
        .all(&db)
        .await?
        .into_iter()
        .map(ProviderEndpoint::from)
        .collect::<Vec<_>>();
    Ok(EndpointPage {
        total,
        endpoints: attach_endpoint_api_keys(&db, endpoints).await?,
        first: first.max(0),
        rows: rows.clamp(1, 200),
    })
}

pub async fn get_endpoint(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
) -> Result<Option<ProviderEndpoint>> {
    let db = postgres_connection(pool);
    let endpoint = load_endpoint_projection(&db, endpoint_id).await?;
    match endpoint {
        Some(endpoint) => Ok(attach_endpoint_api_keys(&db, vec![endpoint])
            .await?
            .into_iter()
            .next()),
        None => Ok(None),
    }
}

fn parse_native_api(value: &str) -> NativeApi {
    serde_json::from_value(serde_json::Value::String(value.to_string())).unwrap_or(NativeApi::Chat)
}

pub(crate) async fn attach_route_config_credentials(
    pool: &PgPool,
    mut routes: Vec<RouteConfig>,
) -> Result<Vec<RouteConfig>> {
    let rows_by_endpoint = list_endpoint_api_keys_by_endpoint_id(
        pool,
        &routes
            .iter()
            .map(|route| route.route_id)
            .collect::<Vec<_>>(),
    )
    .await?;
    // Issue #1004 P1: one existence query hydrates the non-secret
    // subscription eligibility flag for every route. The token values stay
    // in the token table and are resolved only in the authentication path.
    let subscription_endpoints = list_oauth_token_endpoint_ids(pool).await?;
    for route in &mut routes {
        route.api_keys = rows_by_endpoint
            .get(&route.route_id)
            .cloned()
            .unwrap_or_default();
        route.has_oauth_token = subscription_endpoints.contains(&route.route_id);
    }
    Ok(routes)
}

/// Issue #1004 P1: endpoint ids that store a ChatGPT OAuth token. Existence
/// only — no secret is read, and the query reuses the existing token-id
/// listing so no schema or SQL change is needed.
pub(crate) async fn list_oauth_token_endpoint_ids(pool: &PgPool) -> Result<HashSet<uuid::Uuid>> {
    use crate::db::entities::postgres::endpoint_oauth_tokens as tokens;
    let rows = tokens::Entity::find()
        .select_only()
        .column(tokens::Column::EndpointId)
        .filter(tokens::Column::RefreshToken.is_not_null())
        .into_tuple::<uuid::Uuid>()
        .all(&postgres_connection(pool))
        .await
        .map_err(crate::db::seaorm::native_error)?;
    Ok(rows.into_iter().collect())
}
