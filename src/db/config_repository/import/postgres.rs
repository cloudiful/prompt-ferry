//! PostgreSQL configuration import.
//!
//! Replaces the restorable configuration domains inside one transaction. Every
//! statement is built dynamically because the archive carries arbitrary row
//! counts and the destination is a fixed, known table set; values are always
//! bound, never interpolated. Deleting and re-inserting the archive's rows
//! (with their original identifiers) makes the destination match the archive
//! while leaving request/usage/billing/approval/session rows untouched.
//!
//! The domain inserts that live outside this file (routes targets, MCP,
//! relays, settings, redaction) share the same transaction through
//! [`super::postgres_domains`].

use anyhow::{Context, Result};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;

use super::super::snapshot::{EndpointSnapshot, ModelRouteSnapshot, SnapshotDomains};
use super::postgres_domains;

/// Child-before-parent order; deleting an MCP server or endpoint first keeps
/// the cascade graph simple and never touches non-configuration tables.
const DELETE_ORDER: [&str; 12] = [
    "mcp_credentials",
    "mcp_servers",
    "model_route_targets",
    "model_endpoint_rules",
    "endpoint_api_keys",
    "endpoint_oauth_tokens",
    "provider_endpoints",
    "client_keys",
    "managed_relays",
    "user_redaction_configs",
    "worker_settings",
    "users",
];

pub(super) async fn apply(
    pool: &sqlx::PgPool,
    manager: &crate::relay_secrets::RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .context("begin configuration import transaction")?;
    let conn = &mut *tx;
    delete_configuration(conn).await?;
    insert_users(conn, domains).await?;
    insert_client_keys(conn, domains).await?;
    insert_endpoints(conn, domains).await?;
    insert_endpoint_api_keys(conn, domains).await?;
    insert_endpoint_oauth(conn, domains).await?;
    insert_route_rules(conn, domains).await?;
    postgres_domains::insert_route_targets(conn, domains).await?;
    postgres_domains::insert_mcp_servers(conn, domains).await?;
    postgres_domains::insert_mcp_credentials(conn, domains).await?;
    postgres_domains::insert_relays(conn, manager, domains).await?;
    postgres_domains::insert_settings(conn, manager, domains).await?;
    postgres_domains::insert_user_redaction_configs(conn, domains).await?;
    tx.commit()
        .await
        .context("commit configuration import transaction")?;
    Ok(())
}

pub(super) async fn run(conn: &mut PgConnection, mut query: QueryBuilder<Postgres>) -> Result<()> {
    query
        .build()
        .execute(&mut *conn)
        .await
        .context("configuration import statement failed")?;
    Ok(())
}

async fn delete_configuration(conn: &mut PgConnection) -> Result<()> {
    for table in DELETE_ORDER {
        run(conn, QueryBuilder::new(format!("DELETE FROM {table}"))).await?;
    }
    Ok(())
}

/// The archive stores PostgreSQL client keys as a UUID derived from the
/// bigint identity; recover the original identity so usage references survive.
fn client_key_id(key_id: Uuid) -> i64 {
    key_id.as_u64_pair().0 as i64
}

async fn insert_users(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.users.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO users (user_id, login_name, password_hash, display_name, is_admin, \
         is_active, created_at, updated_at) ",
    );
    query.push_values(&domains.users, |mut row, user| {
        row.push_bind(user.user_id)
            .push_bind(user.login_name.clone())
            .push_bind(user.password_hash.clone().unwrap_or_default())
            .push_bind(user.display_name.clone())
            .push_bind(user.is_admin)
            .push_bind(user.is_active)
            .push_bind(user.created_at)
            .push_bind(user.updated_at);
    });
    run(conn, query).await
}

async fn insert_client_keys(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.client_keys.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO client_keys (key_id, user_id, key_prefix, key_hash, label, enabled, secret) ",
    );
    query.push_values(&domains.client_keys, |mut row, key| {
        let hash = key
            .key_hash
            .clone()
            .or_else(|| key.secret.as_deref().map(crate::keys::hash_client_key));
        row.push_bind(client_key_id(key.key_id))
            .push_bind(key.user_id)
            .push_bind(key.key_prefix.clone())
            .push_bind(hash)
            .push_bind(key.label.clone())
            .push_bind(key.enabled)
            .push_bind(key.secret.clone());
    });
    run(conn, query).await
}

async fn insert_endpoints(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.endpoints.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO provider_endpoints (endpoint_id, scope, owner_user_id, name, base_url, \
         native_api, native_api_source, api_key, enabled, key_lb_enabled, provider, \
         provider_region, mcp_enabled, service_tier, proxy_url, active_windows, admin_api_key, \
         created_at, updated_at) ",
    );
    query.push_values(&domains.endpoints, |mut row, endpoint| {
        row.push_bind(endpoint.endpoint_id)
            .push_bind(endpoint.scope.clone())
            .push_bind(endpoint.owner_user_id)
            .push_bind(endpoint.name.clone())
            .push_bind(endpoint.base_url.clone())
            .push_bind(endpoint.native_api.clone())
            .push_bind(endpoint.native_api_source.clone())
            .push_bind(fallback_api_key(endpoint))
            .push_bind(endpoint.enabled)
            .push_bind(endpoint.key_lb_enabled)
            .push_bind(endpoint.provider.clone())
            .push_bind(endpoint.provider_region.clone())
            .push_bind(endpoint.mcp_enabled)
            .push_bind(endpoint.service_tier.clone())
            .push_bind(endpoint.proxy_url.clone())
            .push_bind(endpoint.active_windows.clone())
            .push_bind(endpoint.admin_api_key.clone())
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

fn fallback_api_key(endpoint: &EndpointSnapshot) -> String {
    endpoint
        .api_key
        .clone()
        .or_else(|| {
            endpoint
                .api_keys
                .first()
                .and_then(|key| key.api_key.clone())
        })
        .unwrap_or_default()
}

async fn insert_endpoint_api_keys(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    let mut keys = Vec::new();
    for endpoint in &domains.endpoints {
        for key in &endpoint.api_keys {
            keys.push((endpoint, key));
        }
    }
    if keys.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, \
         enabled, created_at, updated_at) ",
    );
    query.push_values(keys, |mut row, (endpoint, key)| {
        row.push_bind(key.key_id)
            .push_bind(endpoint.endpoint_id)
            .push_bind(key.key_label.clone())
            .push_bind(key.api_key.clone().unwrap_or_default())
            .push_bind(key.position)
            .push_bind(key.enabled)
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

async fn insert_endpoint_oauth(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    let tokens: Vec<&EndpointSnapshot> = domains
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.oauth.is_some())
        .collect();
    if tokens.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token, expires_at, \
         created_at, updated_at) ",
    );
    query.push_values(tokens, |mut row, endpoint| {
        let oauth = endpoint
            .oauth
            .as_ref()
            .expect("filtered to oauth endpoints");
        row.push_bind(endpoint.endpoint_id)
            .push_bind(oauth.access_token.clone())
            .push_bind(oauth.refresh_token.clone())
            .push_bind(oauth.expires_at)
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

/// Route rules reference their first target's endpoint through the legacy
/// `endpoint_id` column; rules without targets cannot be restored and are
/// skipped (the export never produces them).
async fn insert_route_rules(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    let routes: Vec<&ModelRouteSnapshot> = domains
        .model_routes
        .iter()
        .filter(|route| !route.targets.is_empty())
        .collect();
    if routes.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO model_endpoint_rules (rule_id, scope, owner_user_id, model_pattern, \
         routing_strategy, endpoint_id, priority, enabled, created_at, updated_at) ",
    );
    query.push_values(routes.iter(), |mut row, route| {
        let now = route
            .updated_at
            .unwrap_or(route.created_at.unwrap_or_else(chrono::Utc::now));
        row.push_bind(route.rule_id)
            .push_bind(route.scope.clone())
            .push_bind(route.owner_user_id)
            .push_bind(route.model_pattern.clone())
            .push_bind(route.routing_strategy.clone())
            .push_bind(route.targets[0].endpoint_id)
            .push_bind(100_i32)
            .push_bind(route.enabled)
            .push_bind(route.created_at.unwrap_or(now))
            .push_bind(now);
    });
    run(conn, query).await
}
