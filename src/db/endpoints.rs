use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashMap;

use crate::{
    config::NativeApi,
    db::types::{
        EndpointApiKey, EndpointCreate, EndpointPage, ProviderEndpoint, ProviderEndpointRow,
        RouteConfig,
    },
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
            responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: crate::db::RouteSelectionReason::Default,
        })
        .collect::<Vec<_>>();
    attach_route_config_api_keys(pool, routes).await
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

pub async fn create_endpoint(pool: &PgPool, input: EndpointCreate) -> Result<ProviderEndpoint> {
    let endpoint = sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/create_endpoint.sql",
        input.scope,
        input.owner_user_id,
        input.name,
        input.base_url,
        input.native_api.as_str(),
        input.native_api_source.as_str(),
        input.daily_max_requests,
        input.monthly_max_requests,
        input.api_key,
        input.key_lb_enabled,
        input.enabled,
    )
    .fetch_one(pool)
    .await
    .map(ProviderEndpoint::from)?;
    replace_endpoint_api_keys(pool, endpoint.endpoint_id, &input.api_keys).await?;
    Ok(attach_endpoint_api_keys(pool, vec![endpoint])
        .await?
        .remove(0))
}

pub async fn update_endpoint(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
    input: EndpointCreate,
) -> Result<Option<ProviderEndpoint>> {
    let endpoint = sqlx::query_file_as!(
        ProviderEndpointRow,
        "src/sql/endpoints/update_endpoint.sql",
        endpoint_id,
        input.scope,
        input.owner_user_id,
        input.name,
        input.base_url,
        input.native_api.as_str(),
        input.native_api_source.as_str(),
        input.daily_max_requests,
        input.monthly_max_requests,
        input.api_key,
        input.key_lb_enabled,
        input.enabled,
    )
    .fetch_optional(pool)
    .await?
    .map(ProviderEndpoint::from);
    if let Some(endpoint) = endpoint {
        replace_endpoint_api_keys(pool, endpoint.endpoint_id, &input.api_keys).await?;
        Ok(Some(
            attach_endpoint_api_keys(pool, vec![endpoint])
                .await?
                .remove(0),
        ))
    } else {
        Ok(None)
    }
}

pub async fn delete_endpoint(pool: &PgPool, endpoint_id: uuid::Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/endpoints/delete_endpoint.sql", endpoint_id)
        .execute(pool)
        .await?;
    if result.rows_affected() > 0 {
        super::routes::cleanup_orphan_model_routes(pool).await?;
    }
    Ok(result.rows_affected() > 0)
}

pub async fn set_user_endpoint_setting(
    pool: &PgPool,
    user_id: i64,
    endpoint_id: Option<uuid::Uuid>,
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

fn parse_native_api(value: &str) -> NativeApi {
    serde_json::from_value(serde_json::Value::String(value.to_string())).unwrap_or(NativeApi::Chat)
}

async fn attach_endpoint_api_keys(
    pool: &PgPool,
    mut endpoints: Vec<ProviderEndpoint>,
) -> Result<Vec<ProviderEndpoint>> {
    let rows_by_endpoint = list_endpoint_api_keys_by_endpoint_id(
        pool,
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
                key_id: uuid::Uuid::nil(),
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

pub(crate) async fn attach_route_config_api_keys(
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
    for route in &mut routes {
        route.api_keys = rows_by_endpoint
            .get(&route.route_id)
            .cloned()
            .unwrap_or_default();
    }
    Ok(routes)
}

pub(crate) async fn list_endpoint_api_keys_by_endpoint_id(
    pool: &PgPool,
    endpoint_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, Vec<EndpointApiKey>>> {
    let rows = if endpoint_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_file_as!(
            EndpointApiKey,
            "src/sql/endpoints/list_endpoint_api_keys.sql",
            endpoint_ids,
        )
        .fetch_all(pool)
        .await?
    };
    let mut rows_by_endpoint = HashMap::<uuid::Uuid, Vec<EndpointApiKey>>::new();
    for row in rows {
        rows_by_endpoint
            .entry(row.endpoint_id)
            .or_default()
            .push(row);
    }
    Ok(rows_by_endpoint)
}

async fn replace_endpoint_api_keys(
    pool: &PgPool,
    endpoint_id: uuid::Uuid,
    api_keys: &[crate::db::types::EndpointApiKeyCreate],
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/endpoints/delete_endpoint_api_keys.sql",
        endpoint_id
    )
    .execute(pool)
    .await?;
    for api_key in api_keys {
        sqlx::query_file!(
            "src/sql/endpoints/insert_endpoint_api_key.sql",
            endpoint_id,
            api_key.key_label.as_str(),
            api_key.api_key.as_str(),
            api_key.position,
            api_key.enabled,
        )
        .fetch_one(pool)
        .await?;
    }
    Ok(())
}
