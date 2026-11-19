//! PostgreSQL configuration snapshot reader.
//!
//! Reads only restorable configuration: users (Argon2id hashes, never
//! plaintext passwords), client keys, provider endpoints with their API keys,
//! proxy, OpenAI admin key and ChatGPT OAuth token, model routes and targets,
//! MCP servers and credentials, personal MCP OAuth grants, restorable worker
//! settings, and per-user redaction configs. Deployment-only state — the raw
//! object-store configuration, the shared Valkey coordination row and other
//! internal coordination rows — is never read into an archive.
//! Request/usage/session tables are never touched.
//!
//! The orchestration stays here; the pure row-to-record conversions live in
//! [`super::snapshot_pg_map`].

use anyhow::{Context, Result};

use super::snapshot::{
    ClientKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot, McpCredentialSnapshot,
    McpServerSnapshot, ModelRouteSnapshot, SettingSnapshot, SnapshotDomains,
    UserEndpointSettingSnapshot, UserRedactionConfigSnapshot, UserSnapshot,
};
use super::snapshot_pg_map;
use crate::{
    db::{self, McpOAuthGrantSnapshot, config_repository::ConfigRepository},
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
    let mcp_oauth = read_mcp_oauth_grants(&pool, manager, &mcp_servers).await?;
    let settings = read_settings(&pool).await?;
    let user_redaction_configs = read_user_redaction_configs(&pool).await?;
    let user_endpoint_settings = read_user_endpoint_settings(&pool).await?;

    Ok(SnapshotDomains {
        users,
        client_keys,
        endpoints,
        model_routes,
        mcp_servers,
        mcp_credentials,
        settings,
        user_redaction_configs,
        mcp_oauth,
        user_endpoint_settings,
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
    let endpoints = db::list_endpoints(pool).await?;
    let endpoint_ids: Vec<uuid::Uuid> = endpoints
        .iter()
        .map(|endpoint| endpoint.endpoint_id)
        .collect();
    // Read the stored key rows in one batch before any runtime hydration: the
    // endpoints returned above carry a virtual fallback key for a legacy
    // primary-only endpoint, and that fabricated identity must never enter an
    // archive. Persisted nil UUIDs are real rows and stay untouched.
    let mut persisted_api_keys =
        crate::db::endpoints::list_endpoint_api_keys_by_endpoint_id(pool, &endpoint_ids).await?;
    let mut snapshots = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
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
        let api_keys = persisted_api_keys
            .remove(&endpoint.endpoint_id)
            .unwrap_or_default();
        snapshots.push(snapshot_pg_map::convert_endpoint(
            endpoint,
            admin_api_key,
            oauth,
            api_keys,
        ));
    }
    Ok(snapshots)
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

/// Read the restorable worker settings.
///
/// The raw object-store configuration row is deployment state: it is excluded
/// here so a format-3 export never carries S3 credentials or settings, even
/// though the runtime API still owns the row until the storage cutover.
async fn read_settings(pool: &sqlx::PgPool) -> Result<Vec<SettingSnapshot>> {
    let settings = super::settings::list_all_settings_postgres(pool).await?;
    Ok(settings
        .into_iter()
        .filter(|setting| {
            setting.key != super::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY
        })
        .collect())
}

/// Read the personal MCP OAuth grants of every server that can own one.
///
/// The grant material is decrypted with the source secret manager, so the
/// archive carries plaintext that the destination seals again with its own
/// master secret; no source ciphertext, nonce or key version travels with it.
/// A server without an owner cannot own a personal grant, and the grant table
/// itself is keyed by the server and its owner.
async fn read_mcp_oauth_grants(
    pool: &sqlx::PgPool,
    manager: Option<&RelaySecretManager>,
    servers: &[McpServerSnapshot],
) -> Result<Vec<McpOAuthGrantSnapshot>> {
    let Some(manager) = manager else {
        return Ok(Vec::new());
    };
    let mut grants = Vec::new();
    for server in servers {
        // A personal grant belongs to the owner of its server, so a server
        // without an owner cannot hold one.
        let Some(owner_user_id) = server.owner_user_id else {
            continue;
        };
        let Some(secrets) =
            db::mcp_oauth::load_credential(pool, manager, server.server_id, owner_user_id).await?
        else {
            continue;
        };
        grants.push(snapshot_pg_map::convert_mcp_oauth_grant(secrets));
    }
    Ok(grants)
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

/// Explicit per-user endpoint selections, including rows whose endpoint is
/// NULL. The nullable column is read directly so "no explicit endpoint" is
/// archived distinctly from "no row".
async fn read_user_endpoint_settings(
    pool: &sqlx::PgPool,
) -> Result<Vec<UserEndpointSettingSnapshot>> {
    let settings = sqlx::query_file_as!(
        UserEndpointSettingSnapshot,
        "src/sql/settings/list_user_endpoint_settings_for_backup.sql"
    )
    .fetch_all(pool)
    .await
    .context("failed to read the per-user endpoint preferences")?;
    Ok(settings)
}
