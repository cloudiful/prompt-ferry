//! PostgreSQL configuration snapshot reader.
//!
//! Reads only restorable configuration: users (Argon2id hashes, never
//! plaintext passwords), client keys, provider endpoints with their API keys,
//! proxy, OpenAI admin key and ChatGPT OAuth token, model routes and targets,
//! MCP servers and credentials, managed relays with their sealed secrets, all
//! worker settings, per-user redaction configs, and the raw object-store
//! configuration. Request/usage/billing/approval/session tables are never
//! touched.
//!
//! The orchestration stays here; the pure row-to-record conversions live in
//! [`super::snapshot_pg_map`].

use anyhow::{Context, Result};

use super::snapshot::{
    ClientKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot, McpCredentialSnapshot,
    McpServerSnapshot, ModelRouteSnapshot, RelaySnapshot, SettingSnapshot, SnapshotDomains,
    UserRedactionConfigSnapshot, UserSnapshot,
};
use super::snapshot_pg_map;
use crate::{
    db::{self, config_repository::ConfigRepository},
    relay_secrets::RelaySecretManager,
};

pub(super) async fn read(
    repository: &ConfigRepository,
    manager: Option<&RelaySecretManager>,
    users: Vec<UserSnapshot>,
) -> Result<SnapshotDomains> {
    let pool = repository
        .as_postgres()
        .context("postgres configuration snapshot requires a postgres repository")?
        .clone();

    let client_keys = read_client_keys(&pool, &users).await?;
    let endpoints = read_endpoints(repository, &pool).await?;
    let model_routes = read_model_routes(&pool).await?;
    let mcp_servers = read_mcp_servers(&pool).await?;
    let mcp_credentials = read_mcp_credentials(repository).await?;
    let relays = read_relays(&pool, manager).await?;
    let settings = read_settings(&pool).await?;
    let user_redaction_configs = read_user_redaction_configs(&pool).await?;
    let raw_object_store = read_raw_object_store(&pool, manager).await?;

    Ok(SnapshotDomains {
        users,
        client_keys,
        endpoints,
        model_routes,
        mcp_servers,
        mcp_credentials,
        relays,
        settings,
        user_redaction_configs,
        raw_object_store,
    })
}

async fn read_client_keys(
    pool: &sqlx::PgPool,
    users: &[UserSnapshot],
) -> Result<Vec<ClientKeySnapshot>> {
    let mut keys = Vec::new();
    for user in users {
        for key in db::list_client_keys(pool, user.user_id).await? {
            keys.push(snapshot_pg_map::convert_client_key(key));
        }
    }
    Ok(keys)
}

async fn read_endpoints(
    repository: &ConfigRepository,
    pool: &sqlx::PgPool,
) -> Result<Vec<EndpointSnapshot>> {
    let mut endpoints = Vec::new();
    for endpoint in db::list_endpoints(pool).await? {
        let oauth = repository
            .get_endpoint_oauth_token(endpoint.endpoint_id)
            .await
            .context("failed to read endpoint oauth token for the configuration snapshot")?
            .map(|token| EndpointOAuthSnapshot {
                access_token: token.access_token,
                refresh_token: token.refresh_token,
                expires_at: token.expires_at,
            });
        let admin_api_key = repository
            .endpoint_admin_api_key(endpoint.endpoint_id)
            .await
            .context("failed to read endpoint admin api key for the configuration snapshot")?;
        endpoints.push(snapshot_pg_map::convert_endpoint(
            endpoint,
            admin_api_key,
            oauth,
        ));
    }
    Ok(endpoints)
}

async fn read_model_routes(pool: &sqlx::PgPool) -> Result<Vec<ModelRouteSnapshot>> {
    Ok(db::list_model_endpoint_rules(pool)
        .await?
        .into_iter()
        .map(snapshot_pg_map::convert_route)
        .collect())
}

async fn read_mcp_servers(pool: &sqlx::PgPool) -> Result<Vec<McpServerSnapshot>> {
    Ok(db::list_mcp_servers(pool)
        .await?
        .into_iter()
        .map(snapshot_pg_map::convert_mcp_server)
        .collect())
}

async fn read_mcp_credentials(repository: &ConfigRepository) -> Result<Vec<McpCredentialSnapshot>> {
    Ok(repository
        .list_all_mcp_credentials()
        .await?
        .into_iter()
        .map(snapshot_pg_map::convert_mcp_credential)
        .collect())
}

async fn read_relays(
    pool: &sqlx::PgPool,
    manager: Option<&RelaySecretManager>,
) -> Result<Vec<RelaySnapshot>> {
    let mut relays = Vec::new();
    for row in db::list_managed_relays(pool).await? {
        let secrets = super::relays_map::managed_secrets_from_row(&row);
        relays.push(snapshot_pg_map::convert_relay(row, secrets, manager)?);
    }
    Ok(relays)
}

async fn read_settings(pool: &sqlx::PgPool) -> Result<Vec<SettingSnapshot>> {
    super::settings::list_all_settings_postgres(pool).await
}

async fn read_user_redaction_configs(
    pool: &sqlx::PgPool,
) -> Result<Vec<UserRedactionConfigSnapshot>> {
    let configs = db::list_user_redaction_configs(pool).await?;
    let mut snapshots = configs
        .into_iter()
        .map(|(user_id, config)| {
            Ok(UserRedactionConfigSnapshot {
                user_id,
                config: serde_json::to_value(config)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    snapshots.sort_by_key(|snapshot| snapshot.user_id);
    Ok(snapshots)
}

async fn read_raw_object_store(
    pool: &sqlx::PgPool,
    manager: Option<&RelaySecretManager>,
) -> Result<Option<serde_json::Value>> {
    let Some(manager) = manager else {
        return Ok(None);
    };
    let Some(config) = db::get_raw_object_store_config(pool, manager).await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::to_value(config)?))
}
