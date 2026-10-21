use anyhow::Result;
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

pub async fn list_endpoints(pool: &PgPool) -> Result<Vec<ProviderEndpoint>> {
    let endpoints =
        sqlx::query_file_as!(ProviderEndpointRow, "src/sql/endpoints/list_endpoints.sql",)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(ProviderEndpoint::from)
            .collect::<Vec<_>>();
    attach_endpoint_api_keys(pool, endpoints).await
}

pub async fn list_visible_endpoints(pool: &PgPool, user_id: i64) -> Result<Vec<RouteConfig>> {
    let rows = sqlx::query_file!("src/sql/endpoints/list_visible_endpoints.sql", user_id)
        .fetch_all(pool)
        .await?;

    let routes = rows
        .into_iter()
        .map(|row| RouteConfig {
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
    let total = sqlx::query_file!("src/sql/endpoints/count_endpoints.sql")
        .fetch_one(pool)
        .await?
        .total;
    let endpoints = sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/list_endpoints_page.sql",
        first.max(0),
        rows.clamp(1, 200),
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ProviderEndpoint::from)
    .collect::<Vec<_>>();
    Ok(EndpointPage {
        total,
        endpoints: attach_endpoint_api_keys(pool, endpoints).await?,
        first: first.max(0),
        rows: rows.clamp(1, 200),
    })
}

pub async fn get_endpoint(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
) -> Result<Option<ProviderEndpoint>> {
    let endpoint = sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/get_endpoint.sql",
        endpoint_id,
    )
    .fetch_optional(pool)
    .await?
    .map(ProviderEndpoint::from);
    match endpoint {
        Some(endpoint) => Ok(attach_endpoint_api_keys(pool, vec![endpoint])
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
    let rows = sqlx::query_file!("src/sql/endpoints/list_endpoint_oauth_token_ids.sql")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|row| row.endpoint_id).collect())
}
