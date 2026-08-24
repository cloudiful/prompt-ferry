use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool, sqlite::Sqlite};
use uuid::Uuid;

use super::write::{self, EncryptedConfig};
use super::{
    BootstrapSeed, ClientKeyConfig, EndpointApiKeyConfig, ManagedRelayConfig, ModelRouteConfig,
    ModelRouteTargetConfig, ProviderEndpointConfig, ReplaySnapshotUpsertOutcome, Result,
    SettingConfig, StandaloneConfig, StandaloneConfigError, StandaloneReplaySnapshotRecord,
    StandaloneUsageSummaryRecord, rows,
};
use crate::relay_secrets::RelaySecretManager;

const CURRENT_SCHEMA_VERSION: i64 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapOutcome {
    pub seeded: bool,
}

pub struct StandaloneConfigStore {
    pool: SqlitePool,
    path: PathBuf,
}

impl std::fmt::Debug for StandaloneConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StandaloneConfigStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl StandaloneConfigStore {
    /// Open or create a local SQLite configuration file and apply standalone migrations.
    ///
    /// The parent directory must already exist. A missing file is created; malformed,
    /// inaccessible, or future-version files return an explicit error. Secret-bearing
    /// operations separately require the caller to provide the configured
    /// `RelaySecretManager`; this store never creates or persists a master key.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "database path",
                message: "must not be empty".to_string(),
            });
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(StandaloneConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "SQLite parent directory does not exist: {}",
                        parent.display()
                    ),
                )));
            }
        }
        let pool = crate::db::connect_sqlite(path).await?;
        let store = Self {
            pool,
            path: path.to_path_buf(),
        };
        if let Err(error) = store.migrate().await {
            store.pool.close().await;
            return Err(error);
        }
        Ok(store)
    }

    pub async fn migrate(&self) -> Result<()> {
        crate::db::migrate_standalone(&self.pool)
            .await
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!("migration failed: {error}"))
            })?;
        let row = standalone_query!("src/sql/standalone/schema_version.sql")
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "schema metadata query failed: {error}"
                ))
            })?;
        let version = row
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase("schema version row is missing".to_string())
            })?
            .try_get::<i64, _>("schema_version")
            .map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "schema version value is invalid: {error}"
                ))
            })?;
        if version != CURRENT_SCHEMA_VERSION {
            return Err(StandaloneConfigError::UnsupportedSchemaVersion {
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    // ---- Row-level reads for the unified configuration repository. ----

    pub async fn list_endpoints_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ProviderEndpointConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_endpoints.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let endpoint_rows = standalone_query!("src/sql/standalone/list_endpoints_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut endpoints = Vec::with_capacity(endpoint_rows.len());
        for row in endpoint_rows {
            let (mut endpoint, envelope) = rows::endpoint(&row)?;
            endpoint.api_key =
                write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint is missing its API key".to_string(),
                    )
                })?;
            endpoints.push(endpoint);
        }
        Ok((total, endpoints))
    }

    pub async fn get_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<ProviderEndpointConfig>> {
        let row = standalone_query!("src/sql/standalone/get_endpoint.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut endpoint, envelope) = rows::endpoint(&row)?;
        endpoint.api_key =
            write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase(
                    "endpoint is missing its API key".to_string(),
                )
            })?;
        let key_rows = standalone_query!("src/sql/standalone/list_endpoint_keys_for.sql")
            .bind(endpoint_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        for row in key_rows {
            let (mut key, envelope) = rows::endpoint_key(&row)?;
            key.api_key = manager.decrypt(&envelope)?;
            endpoint.api_keys.push(key);
        }
        Ok(Some(endpoint))
    }

    pub async fn list_endpoint_keys_for(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Vec<EndpointApiKeyConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_endpoint_keys_for.sql")
            .bind(endpoint_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut key, envelope) = rows::endpoint_key(&row)?;
                key.api_key = manager.decrypt(&envelope)?;
                Ok(key)
            })
            .collect()
    }

    pub async fn list_mcp_servers(
        &self,
        manager: &RelaySecretManager,
    ) -> Result<Vec<crate::db::McpServer>> {
        let rows = standalone_query!("src/sql/standalone/list_mcp_servers.sql")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| self.mcp_server_from_row(manager, &row))
            .collect()
    }

    pub async fn list_mcp_servers_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<crate::db::McpServer>)> {
        let total = standalone_query!("src/sql/standalone/count_mcp_servers.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_mcp_servers_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let servers = rows
            .iter()
            .map(|row| self.mcp_server_from_row(manager, row))
            .collect::<Result<Vec<_>>>()?;
        Ok((total, servers))
    }

    pub async fn list_user_mcp_servers_page(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<crate::db::McpServer>)> {
        let total = standalone_query!("src/sql/standalone/count_user_mcp_servers.sql")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_user_mcp_servers_page.sql")
            .bind(user_id)
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let servers = rows
            .iter()
            .map(|row| self.mcp_server_from_row(manager, row))
            .collect::<Result<Vec<_>>>()?;
        Ok((total, servers))
    }

    pub async fn list_visible_mcp_servers(
        &self,
        manager: &RelaySecretManager,
        user_id: Option<i64>,
    ) -> Result<Vec<crate::db::McpServer>> {
        let rows = standalone_query!("src/sql/standalone/list_visible_mcp_servers.sql")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| self.mcp_server_from_row(manager, &row))
            .collect()
    }

    pub async fn get_mcp_server(
        &self,
        manager: &RelaySecretManager,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server.sql")
            .bind(server_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_user_mcp_server(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        server_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_user_mcp_server.sql")
            .bind(server_id.to_string())
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_mcp_server_by_name(
        &self,
        manager: &RelaySecretManager,
        name: &str,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server_by_name.sql")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn get_mcp_server_by_source_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint_id: uuid::Uuid,
    ) -> Result<Option<crate::db::McpServer>> {
        let row = standalone_query!("src/sql/standalone/get_mcp_server_by_source_endpoint.sql")
            .bind(endpoint_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref()
            .map(|row| self.mcp_server_from_row(manager, row))
            .transpose()
    }

    pub async fn save_mcp_server(
        &self,
        manager: &RelaySecretManager,
        server_id: uuid::Uuid,
        input: &crate::db::McpServerInput,
        existing: Option<&crate::db::McpServer>,
    ) -> Result<crate::db::McpServer> {
        let env = manager.encrypt(&serde_json::to_string(&input.env_json)?)?;
        let bearer_tokens = manager.encrypt(&serde_json::to_string(&input.bearer_tokens_json)?)?;
        let now = chrono::Utc::now();
        standalone_query!("src/sql/standalone/save_mcp_server.sql")
            .bind(server_id.to_string())
            .bind(input.source_endpoint_id.map(|id| id.to_string()))
            .bind(&input.scope)
            .bind(input.owner_user_id)
            .bind(&input.name)
            .bind(&input.aggregate_naming_mode)
            .bind(&input.transport)
            .bind(&input.url)
            .bind(&input.command)
            .bind(serde_json::to_string(&input.args)?)
            .bind(serde_json::to_string(&input.http_headers_json)?)
            .bind(&input.tool_filter_mode)
            .bind(serde_json::to_string(&input.allowed_tools)?)
            .bind(serde_json::to_string(&input.disabled_tools)?)
            .bind(serde_json::to_string(&input.disabled_resources)?)
            .bind(input.daily_max_requests)
            .bind(input.monthly_max_requests)
            .bind(i64::from(input.enabled))
            .bind(input.timeout_ms)
            .bind(&input.lifecycle_policy)
            .bind(&input.lifecycle_manual_protocol_version)
            .bind(existing.and_then(|server| server.lifecycle_learned_mode.as_deref()))
            .bind(existing.and_then(|server| server.lifecycle_learned_protocol_version.as_deref()))
            .bind(
                existing
                    .map(|server| server.lifecycle_learned_for_updated_at)
                    .flatten()
                    .map(|value| value.to_rfc3339()),
            )
            .bind(
                existing
                    .map(|server| server.lifecycle_learned_at)
                    .flatten()
                    .map(|value| value.to_rfc3339()),
            )
            .bind(env.ciphertext)
            .bind(env.nonce)
            .bind(i64::from(env.key_version))
            .bind(bearer_tokens.ciphertext)
            .bind(bearer_tokens.nonce)
            .bind(i64::from(bearer_tokens.key_version))
            .bind(
                existing
                    .map(|server| server.created_at)
                    .unwrap_or(now)
                    .to_rfc3339(),
            )
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
        self.get_mcp_server(manager, server_id)
            .await?
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase("MCP server missing after save".to_string())
            })
    }

    pub async fn delete_mcp_server(&self, server_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_mcp_server.sql")
            .bind(server_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_mcp_lifecycle_learned(
        &self,
        server: &crate::db::McpServer,
        mode: &str,
        protocol_version: &str,
    ) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/mark_mcp_lifecycle_learned.sql")
            .bind(mode)
            .bind(protocol_version)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(server.server_id.to_string())
            .bind(server.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    fn mcp_server_from_row(
        &self,
        manager: &RelaySecretManager,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<crate::db::McpServer> {
        let (mut server, env, bearer_tokens) = rows::mcp_server(row)?;
        server.env_json = decrypt_json(manager, &env, "MCP environment")?;
        server.bearer_tokens_json = decrypt_json(manager, &bearer_tokens, "MCP bearer tokens")?;
        Ok(server)
    }

    pub async fn list_routes_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ModelRouteConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_model_routes.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let route_rows = standalone_query!("src/sql/standalone/list_routes_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut routes = Vec::with_capacity(route_rows.len());
        for row in route_rows {
            routes.push(rows::route(&row)?);
        }
        for row in standalone_query!("src/sql/standalone/list_route_targets.sql")
            .fetch_all(&self.pool)
            .await?
        {
            let (rule_id, target) = rows::route_target(&row)?;
            routes
                .iter_mut()
                .find(|route| route.rule_id == rule_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "route target references a missing route".to_string(),
                    )
                })?
                .targets
                .push(target);
        }
        Ok((total, routes))
    }

    pub async fn get_route(&self, rule_id: uuid::Uuid) -> Result<Option<ModelRouteConfig>> {
        let row = standalone_query!("src/sql/standalone/get_model_route.sql")
            .bind(rule_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let mut route = rows::route(&row)?;
        for row in standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?
        {
            let (_, target) = rows::route_target(&row)?;
            route.targets.push(target);
        }
        Ok(Some(route))
    }

    pub async fn list_route_targets_for(
        &self,
        rule_id: uuid::Uuid,
    ) -> Result<Vec<ModelRouteTargetConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| Ok(rows::route_target(&row)?.1))
            .collect()
    }

    pub async fn list_relays_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, i64, Vec<ManagedRelayConfig>)> {
        let counts = standalone_query!("src/sql/standalone/count_relays.sql")
            .fetch_one(&self.pool)
            .await?;
        let total = counts.try_get::<i64, _>("total")?;
        let enabled_count = counts
            .try_get::<Option<i64>, _>("enabled_count")?
            .unwrap_or(0);
        let relay_rows = standalone_query!("src/sql/standalone/list_relays_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut relays = Vec::with_capacity(relay_rows.len());
        for row in relay_rows {
            let (mut relay, envelopes) = rows::relay(&row)?;
            relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
            relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
            relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
            relay.bridge_encryption_key = write::decrypt_optional(manager, envelopes[3].as_ref())?;
            relays.push(relay);
        }
        Ok((total, enabled_count, relays))
    }

    pub async fn get_relay(
        &self,
        manager: &RelaySecretManager,
        relay_id: uuid::Uuid,
    ) -> Result<Option<ManagedRelayConfig>> {
        let row = standalone_query!("src/sql/standalone/get_relay.sql")
            .bind(relay_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut relay, envelopes) = rows::relay(&row)?;
        relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
        relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
        relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
        relay.bridge_encryption_key = write::decrypt_optional(manager, envelopes[3].as_ref())?;
        Ok(Some(relay))
    }

    /// Look up a managed relay and return the encrypted envelopes for its
    /// TLS/bridge secrets. Used by update handlers to carry forward
    /// unchanged secrets via the `Keep` patch mode.
    pub async fn get_relay_envelopes(
        &self,
        relay_id: uuid::Uuid,
    ) -> Result<Option<[Option<crate::relay_secrets::EncryptedSecretEnvelope>; 4]>> {
        let row = standalone_query!("src/sql/standalone/get_relay_envelopes.sql")
            .bind(relay_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(super::rows::relay_envelopes(&row)?))
    }

    pub async fn list_client_keys_for(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ClientKeyConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_client_keys_for.sql")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let rows = standalone_query!("src/sql/standalone/list_client_keys_for.sql")
            .bind(user_id)
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            let (mut key, envelope) = rows::client_key(&row)?;
            key.secret = manager.decrypt(&envelope)?;
            keys.push(key);
        }
        Ok((total, keys))
    }

    pub async fn get_client_key(
        &self,
        manager: &RelaySecretManager,
        key_id: uuid::Uuid,
    ) -> Result<Option<ClientKeyConfig>> {
        let row = standalone_query!("src/sql/standalone/get_client_key.sql")
            .bind(key_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let (mut key, envelope) = rows::client_key(&row)?;
        key.secret = manager.decrypt(&envelope)?;
        Ok(Some(key))
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<SettingConfig>> {
        let row = standalone_query!("src/sql/standalone/get_setting.sql")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(rows::setting(&row)?))
    }

    pub async fn load_snapshot(&self, manager: &RelaySecretManager) -> Result<StandaloneConfig> {
        let mut transaction = self.pool.begin().await?;
        let snapshot = StandaloneConfig {
            relays: Self::load_relays(&mut transaction, manager).await?,
            endpoints: Self::load_endpoints(&mut transaction, manager).await?,
            routes: Self::load_routes(&mut transaction).await?,
            client_keys: Self::load_client_keys(&mut transaction, manager).await?,
            settings: Self::load_settings(&mut transaction).await?,
        };
        snapshot.validate()?;
        transaction.commit().await?;
        Ok(snapshot)
    }

    pub async fn replace_snapshot(
        &self,
        manager: &RelaySecretManager,
        snapshot: &StandaloneConfig,
    ) -> Result<()> {
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, snapshot)?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/create_snapshot_endpoint_ids.sql")
            .execute(&mut *transaction)
            .await?;
        standalone_query!("src/sql/standalone/clear_snapshot_endpoint_ids.sql")
            .execute(&mut *transaction)
            .await?;
        for endpoint in &snapshot.endpoints {
            standalone_query!("src/sql/standalone/save_snapshot_endpoint_id.sql")
                .bind(endpoint.endpoint_id.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        write::delete_all(&mut transaction).await?;
        write::insert_all(&mut transaction, &encrypted).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn bootstrap_if_empty(
        &self,
        manager: &RelaySecretManager,
        seed: BootstrapSeed,
    ) -> Result<BootstrapOutcome> {
        let snapshot = seed.into_config()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        let count = standalone_query!("src/sql/standalone/configuration_count.sql")
            .fetch_one(&mut *transaction)
            .await?
            .try_get::<i64, _>("record_count")?;
        if count != 0 {
            transaction.rollback().await?;
            return Ok(BootstrapOutcome { seeded: false });
        }
        write::insert_all(&mut transaction, &encrypted).await?;
        transaction.commit().await?;
        Ok(BootstrapOutcome { seeded: true })
    }

    pub async fn save_relay(
        &self,
        manager: &RelaySecretManager,
        relay: &ManagedRelayConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            relays: vec![relay.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        write::insert_relay(&mut transaction, &encrypted.relays[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_endpoint(
        &self,
        manager: &RelaySecretManager,
        endpoint: &ProviderEndpointConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            endpoints: vec![endpoint.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_endpoint_keys.sql")
            .bind(endpoint.endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_endpoint(&mut transaction, &encrypted.endpoints[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_endpoint_mcp_enabled(
        &self,
        endpoint_id: uuid::Uuid,
        enabled: bool,
    ) -> Result<()> {
        standalone_query!("src/sql/standalone/set_endpoint_mcp_enabled.sql")
            .bind(i64::from(enabled))
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(endpoint_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_route(&self, route: &ModelRouteConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            routes: vec![route.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(route.rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_route(&mut transaction, route).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Insert or replace a single model route and its targets without running
    /// the full snapshot validator. Used by the unified configuration
    /// repository when an endpoint has just been created in the same
    /// transaction context.
    pub async fn save_route_direct(&self, route: &ModelRouteConfig) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(route.rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_route(&mut transaction, route).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn save_client_key(
        &self,
        manager: &RelaySecretManager,
        client_key: &ClientKeyConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            client_keys: vec![client_key.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let encrypted = EncryptedConfig::from_snapshot(manager, &snapshot)?;
        let mut transaction = self.pool.begin().await?;
        write::insert_client_key(&mut transaction, &encrypted.client_keys[0]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_setting(&self, setting: &SettingConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            settings: vec![setting.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        standalone_query!("src/sql/standalone/save_setting.sql")
            .bind(&setting.key)
            .bind(setting.version)
            .bind(serde_json::to_string(&setting.value)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- Row-level mutations for the unified configuration repository. ----

    pub async fn delete_endpoint(&self, endpoint_id: uuid::Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_endpoint_keys.sql")
            .bind(endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let result = standalone_query!("src/sql/standalone/delete_endpoint.sql")
            .bind(endpoint_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let removed = result.rows_affected() > 0;
        transaction.commit().await?;
        Ok(removed)
    }

    pub async fn delete_route(&self, rule_id: uuid::Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let result = standalone_query!("src/sql/standalone/delete_model_route.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let removed = result.rows_affected() > 0;
        transaction.commit().await?;
        Ok(removed)
    }

    pub async fn delete_relay(&self, relay_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_relay.sql")
            .bind(relay_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_client_key(&self, key_id: uuid::Uuid) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_client_key.sql")
            .bind(key_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_setting.sql")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn load_relays(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ManagedRelayConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_relays.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut relay, envelopes) = rows::relay(&row)?;
                relay.relay_ca_pem = write::decrypt_optional(manager, envelopes[0].as_ref())?;
                relay.client_cert_pem = write::decrypt_optional(manager, envelopes[1].as_ref())?;
                relay.client_key_pem = write::decrypt_optional(manager, envelopes[2].as_ref())?;
                relay.bridge_encryption_key =
                    write::decrypt_optional(manager, envelopes[3].as_ref())?;
                Ok(relay)
            })
            .collect()
    }

    async fn load_endpoints(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ProviderEndpointConfig>> {
        let endpoint_rows = standalone_query!("src/sql/standalone/list_endpoints.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let key_rows = standalone_query!("src/sql/standalone/list_endpoint_keys.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let mut endpoints = Vec::with_capacity(endpoint_rows.len());
        for row in endpoint_rows {
            let (mut endpoint, envelope) = rows::endpoint(&row)?;
            endpoint.api_key =
                write::decrypt_optional(manager, envelope.as_ref())?.ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint is missing its API key".to_string(),
                    )
                })?;
            endpoints.push(endpoint);
        }
        for row in key_rows {
            let endpoint_id = rows::uuid(&row, "endpoint_id")?;
            let (mut key, envelope) = rows::endpoint_key(&row)?;
            key.api_key = manager.decrypt(&envelope)?;
            endpoints
                .iter_mut()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "endpoint key references a missing endpoint".to_string(),
                    )
                })?
                .api_keys
                .push(key);
        }
        Ok(endpoints)
    }

    async fn load_routes(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<ModelRouteConfig>> {
        let route_rows = standalone_query!("src/sql/standalone/list_routes.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let target_rows = standalone_query!("src/sql/standalone/list_route_targets.sql")
            .fetch_all(&mut **transaction)
            .await?;
        let mut routes = route_rows
            .into_iter()
            .map(|row| rows::route(&row))
            .collect::<Result<Vec<_>>>()?;
        for row in target_rows {
            let (rule_id, target) = rows::route_target(&row)?;
            routes
                .iter_mut()
                .find(|route| route.rule_id == rule_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "route target references a missing route".to_string(),
                    )
                })?
                .targets
                .push(target);
        }
        Ok(routes)
    }

    async fn load_client_keys(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        manager: &RelaySecretManager,
    ) -> Result<Vec<ClientKeyConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_client_keys.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (mut key, envelope) = rows::client_key(&row)?;
                key.secret = manager.decrypt(&envelope)?;
                Ok(key)
            })
            .collect()
    }

    async fn load_settings(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<Vec<SettingConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_settings.sql")
            .fetch_all(&mut **transaction)
            .await?;
        rows.into_iter().map(|row| rows::setting(&row)).collect()
    }

    // ---- Durable standalone usage summary ledger (Phase 1A). ----

    /// Persist a single compact standalone usage summary. The ledger key is
    /// the independent `event_id` column so retried, replayed, or repeated
    /// lifecycle events for the same request each become a distinct row.
    /// Returns the new row's `event_id`.
    pub async fn insert_usage_summary(
        &self,
        summary: &StandaloneUsageSummaryRecord,
    ) -> Result<i64> {
        let recorded_at = summary.recorded_at.to_rfc3339();
        let redaction_types_json = serde_json::to_string(&summary.redaction_types)?;
        let redaction_fields_json = serde_json::to_string(&summary.redaction_fields)?;
        let result = standalone_query!("src/sql/standalone/insert_usage_summary.sql")
            .bind(summary.request_id.to_string())
            .bind(&summary.event_kind)
            .bind(&summary.category)
            .bind(&summary.state)
            .bind(&summary.path)
            .bind(recorded_at)
            .bind(summary.status)
            .bind(summary.ok.map(i64::from))
            .bind(summary.duration_ms)
            .bind(summary.ttft_ms)
            .bind(&summary.model)
            .bind(&summary.requested_model)
            .bind(&summary.upstream_model)
            .bind(summary.endpoint_id.map(|id| id.to_string()))
            .bind(summary.endpoint_key_id.map(|id| id.to_string()))
            .bind(summary.model_route_rule_id.map(|id| id.to_string()))
            .bind(summary.mcp_server_id.map(|id| id.to_string()))
            .bind(summary.input_tokens)
            .bind(summary.output_tokens)
            .bind(summary.total_tokens)
            .bind(summary.cached_tokens)
            .bind(summary.cache_read_tokens)
            .bind(summary.cache_write_tokens)
            .bind(&summary.error_code)
            .bind(&summary.failure_family)
            .bind(i64::from(summary.redaction_applied))
            .bind(i64::from(summary.redaction_findings_count))
            .bind(i64::from(summary.redaction_replacements_count))
            .bind(redaction_types_json)
            .bind(redaction_fields_json)
            .bind(&summary.route_selection_reason)
            .bind(summary.user_id)
            .bind(summary.client_key_id)
            .bind(&summary.client_key_label)
            .bind(&summary.request_user_agent)
            .bind(&summary.endpoint_key_label)
            .bind(&summary.mcp_server_name)
            .bind(&summary.mcp_protocol_method)
            .bind(&summary.mcp_operation_name)
            .bind(&summary.http_request_content_encoding)
            .bind(i64::from(summary.http_request_compressed))
            .bind(summary.http_request_compressed_bytes)
            .bind(summary.http_request_decompressed_bytes)
            .bind(summary.http_request_compression_ratio)
            .bind(&summary.conversation_source)
            .bind(&summary.client_installation_id)
            .bind(&summary.provider_response_id)
            .bind(&summary.provider_conversation_key)
            .bind(&summary.request_storage_mode)
            .bind(&summary.error_message)
            .bind(i64::from(summary.request_has_previous_response_id))
            .bind(&summary.request_previous_response_id)
            .bind(
                summary
                    .request_previous_response_parent_found
                    .map(i64::from),
            )
            .bind(&summary.request_conversation_key)
            .bind(summary.request_conversation_parent_found.map(i64::from))
            .bind(i64::from(summary.upstream_redaction_enabled))
            .bind(i64::from(summary.response_capture_truncated))
            .execute(&self.pool)
            .await?;
        Ok(result.last_insert_rowid())
    }

    /// Return at most `limit` recent summaries in insertion order (oldest
    /// first). `limit` is clamped to a positive value so a misconfigured
    /// caller cannot request a negative window. A single malformed row
    /// skips with a warning so the rest of the ledger still loads.
    pub async fn list_usage_summaries(
        &self,
        limit: i64,
    ) -> Result<Vec<StandaloneUsageSummaryRecord>> {
        let bounded = limit.clamp(1, 4096);
        let rows = standalone_query!("src/sql/standalone/list_usage_summaries.sql")
            .bind(bounded)
            .fetch_all(&self.pool)
            .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            match parse_usage_summary_row(&row) {
                Ok(Some(record)) => records.push(record),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "skipping malformed standalone usage summary row during list"
                    );
                }
            }
        }
        Ok(records)
    }

    /// Trim the standalone usage ledger so that at most `max_rows` rows
    /// remain. Older rows (by insertion order, expressed via `event_id`)
    /// are removed. Returns the number of rows deleted.
    pub async fn prune_usage_summaries(&self, max_rows: i64) -> Result<i64> {
        let bounded = max_rows.clamp(0, 4096);
        let result = standalone_query!("src/sql/standalone/prune_usage_summaries.sql")
            .bind(bounded)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as i64)
    }

    // ---- Durable standalone replay snapshot store (Phase 1C-b). ----

    /// Monotonically upsert the latest replay snapshot for a single
    /// conversation. The SQL `ON CONFLICT ... DO UPDATE ... WHERE`
    /// gate ensures the existing row is preserved when the incoming
    /// snapshot would regress the stored checkpoint by `(conversation_seq,
    /// base_event_id)` ordering. Raw body payloads remain out of scope:
    /// `prompt_refs_json` carries only role and block-hash references,
    /// validated here as a non-empty JSON array.
    pub async fn upsert_replay_snapshot(
        &self,
        snapshot: &StandaloneReplaySnapshotRecord,
    ) -> Result<ReplaySnapshotUpsertOutcome> {
        if snapshot.conversation_seq <= 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "conversation_seq",
                message: "must be a positive integer".to_string(),
            });
        }
        if snapshot.base_event_id < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "base_event_id",
                message: "must be non-negative".to_string(),
            });
        }
        if snapshot.ref_count < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "ref_count",
                message: "must be non-negative".to_string(),
            });
        }
        if snapshot.byte_size < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "byte_size",
                message: "must be non-negative".to_string(),
            });
        }
        validate_prompt_refs_json(&snapshot.prompt_refs_json)?;
        let updated_at = snapshot.updated_at.to_rfc3339();
        let existed = standalone_query!("src/sql/standalone/get_replay_snapshot.sql")
            .bind(snapshot.conversation_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let result = standalone_query!("src/sql/standalone/upsert_replay_snapshot.sql")
            .bind(snapshot.conversation_id.to_string())
            .bind(snapshot.base_event_id)
            .bind(snapshot.conversation_seq)
            .bind(&snapshot.prompt_refs_json)
            .bind(snapshot.ref_count)
            .bind(snapshot.byte_size)
            .bind(&updated_at)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Ok(ReplaySnapshotUpsertOutcome::Skipped);
        }
        Ok(if existed.is_some() {
            ReplaySnapshotUpsertOutcome::Updated
        } else {
            ReplaySnapshotUpsertOutcome::Inserted
        })
    }

    /// Fetch the latest replay snapshot for a conversation, or `None`
    /// when the conversation has no persisted checkpoint. A malformed
    /// row (bad UUID, unparseable timestamp, invalid prompt-refs JSON)
    /// is reported as `StandaloneConfigError::CorruptDatabase` so the
    /// hydration path can decide whether to skip or fail the entire
    /// load.
    pub async fn get_replay_snapshot(
        &self,
        conversation_id: Uuid,
    ) -> Result<Option<StandaloneReplaySnapshotRecord>> {
        let row = standalone_query!("src/sql/standalone/get_replay_snapshot.sql")
            .bind(conversation_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        parse_replay_snapshot_row(&row)
    }
}

fn decrypt_json(
    manager: &RelaySecretManager,
    envelope: &crate::relay_secrets::EncryptedSecretEnvelope,
    label: &str,
) -> Result<serde_json::Value> {
    let value = manager.decrypt(envelope)?;
    serde_json::from_str(&value).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!("{label} JSON is invalid: {error}"))
    })
}

/// Validate that a `prompt_refs_json` payload is a non-empty JSON array
/// of objects carrying `role` (string) and `block_hash` (string)
/// entries. This is the same shape used by
/// `db::decode_prompt_message_refs` on PostgreSQL, but applied here as
/// a storage-layer guard so a corrupt caller cannot poison the durable
/// snapshot.
fn validate_prompt_refs_json(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must not be empty".to_string(),
        });
    }
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(|error| StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: format!("not valid JSON: {error}"),
        })?;
    let entries = parsed
        .as_array()
        .ok_or_else(|| StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must be a JSON array".to_string(),
        })?;
    if entries.is_empty() {
        return Err(StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must contain at least one entry".to_string(),
        });
    }
    for (index, entry) in entries.iter().enumerate() {
        let object = entry
            .as_object()
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} is not an object"),
            })?;
        let role = object
            .get("role")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} role is not a string"),
            })?;
        if role.is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} role is empty"),
            });
        }
        let block_hash = object
            .get("block_hash")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} block_hash is not a string"),
            })?;
        if block_hash.is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} block_hash is empty"),
            });
        }
    }
    Ok(())
}

/// Map a `standalone_replay_snapshots` row into the storage DTO. Errors
/// from column-level decoding propagate as
/// `StandaloneConfigError::CorruptDatabase` so the caller can decide
/// whether to drop the row or fail the hydration.
fn parse_replay_snapshot_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<StandaloneReplaySnapshotRecord>> {
    let conversation_id: String = row.try_get("conversation_id")?;
    let conversation_id = Uuid::parse_str(&conversation_id).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!(
            "column conversation_id is not a UUID: {error}"
        ))
    })?;
    let base_event_id: i64 = row.try_get("base_event_id")?;
    if base_event_id < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column base_event_id is negative: {base_event_id}"
        )));
    }
    let conversation_seq: i64 = row.try_get("conversation_seq")?;
    if conversation_seq <= 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column conversation_seq is not positive: {conversation_seq}"
        )));
    }
    let prompt_refs_json: String = row.try_get("prompt_refs_json")?;
    let ref_count: i64 = row.try_get("ref_count")?;
    if ref_count < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column ref_count is negative: {ref_count}"
        )));
    }
    let byte_size: i64 = row.try_get("byte_size")?;
    if byte_size < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column byte_size is negative: {byte_size}"
        )));
    }
    let updated_at: String = row.try_get("updated_at")?;
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at)
        .map(|value| value.with_timezone(&chrono::Utc))
        .map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column updated_at is not RFC3339: {error}"
            ))
        })?;
    validate_prompt_refs_json(&prompt_refs_json).map_err(|error| match error {
        StandaloneConfigError::InvalidInput { field, message } => {
            StandaloneConfigError::CorruptDatabase(format!("{field} {message}"))
        }
        other => other,
    })?;
    Ok(Some(StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id,
        conversation_seq: i32::try_from(conversation_seq).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column conversation_seq is outside the i32 range".to_string(),
            )
        })?,
        prompt_refs_json,
        ref_count: i32::try_from(ref_count).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column ref_count is outside the i32 range".to_string(),
            )
        })?,
        byte_size: i32::try_from(byte_size).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column byte_size is outside the i32 range".to_string(),
            )
        })?,
        updated_at,
    }))
}

/// Map a `standalone_usage_summaries` row into the storage DTO. Kept
/// private to this module so the low-level row layout stays internal to
/// the standalone config boundary.
///
/// Returns `Ok(None)` when the row's columns are individually well-typed
/// but carry a value that violates the domain (out-of-range integer,
/// malformed UUID, unparseable timestamp, etc.) so callers can keep the
/// rest of the ledger. Database-level failures still propagate as
/// `StandaloneConfigError::Database` so the surrounding query is not
/// silently retried against a corrupt connection.
fn parse_usage_summary_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<StandaloneUsageSummaryRecord>> {
    match try_parse_usage_summary_row(row) {
        Ok(record) => Ok(Some(record)),
        Err(StandaloneConfigError::CorruptDatabase(reason)) => {
            tracing::warn!(
                reason = %reason,
                "skipping malformed standalone usage summary row during list"
            );
            Ok(None)
        }
        Err(other) => Err(other),
    }
}

fn try_parse_usage_summary_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<StandaloneUsageSummaryRecord> {
    fn required_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String> {
        let value: String = row.try_get(column)?;
        if value.trim().is_empty() {
            return Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is empty"
            )));
        }
        Ok(value)
    }
    fn optional_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<String>> {
        Ok(row.try_get(column)?)
    }
    fn uuid_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<uuid::Uuid> {
        let value: String = row.try_get(column)?;
        uuid::Uuid::parse_str(&value).map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a UUID: {error}"
            ))
        })
    }
    fn uuid_optional(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<uuid::Uuid>> {
        let value = row.try_get::<Option<String>, _>(column)?;
        value
            .map(|value| {
                uuid::Uuid::parse_str(&value).map_err(|error| {
                    StandaloneConfigError::CorruptDatabase(format!(
                        "column {column} is not a UUID: {error}"
                    ))
                })
            })
            .transpose()
    }
    fn optional_i32(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<i32>> {
        let value = row.try_get::<Option<i64>, _>(column)?;
        value
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    StandaloneConfigError::CorruptDatabase(format!(
                        "column {column} is outside the i32 range"
                    ))
                })
            })
            .transpose()
    }
    fn optional_bool(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<bool>> {
        match row.try_get::<Option<i64>, _>(column)? {
            None => Ok(None),
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(other) => Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a boolean: {other}"
            ))),
        }
    }
    fn required_bool(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<bool> {
        match row.try_get::<i64, _>(column)? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a boolean: {value}"
            ))),
        }
    }
    fn parse_timestamp(value: &str, column: &str) -> Result<chrono::DateTime<chrono::Utc>> {
        if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
            return Ok(timestamp.with_timezone(&chrono::Utc));
        }
        let timestamp =
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "column {column} is not a timestamp: {error}"
                ))
            })?;
        Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
            timestamp,
            chrono::Utc,
        ))
    }
    fn sqlite_timestamp(
        row: &sqlx::sqlite::SqliteRow,
        column: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>> {
        let value: String = row.try_get(column)?;
        parse_timestamp(&value, column)
    }
    fn parse_string_list(value: &str, column: &str) -> Result<Vec<String>> {
        serde_json::from_str::<Vec<String>>(value).map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a string array: {error}"
            ))
        })
    }

    let redaction_types_json: String = row.try_get("redaction_types_json")?;
    let redaction_fields_json: String = row.try_get("redaction_fields_json")?;
    Ok(StandaloneUsageSummaryRecord {
        request_id: uuid_required(row, "request_id")?,
        event_kind: required_string(row, "event_kind")?,
        category: required_string(row, "category")?,
        state: required_string(row, "state")?,
        path: required_string(row, "path")?,
        recorded_at: sqlite_timestamp(row, "recorded_at")?,
        status: optional_i32(row, "status")?,
        ok: optional_bool(row, "ok")?,
        duration_ms: row.try_get("duration_ms")?,
        ttft_ms: row.try_get("ttft_ms")?,
        model: optional_string(row, "model")?,
        requested_model: optional_string(row, "requested_model")?,
        upstream_model: optional_string(row, "upstream_model")?,
        endpoint_id: uuid_optional(row, "endpoint_id")?,
        endpoint_key_id: uuid_optional(row, "endpoint_key_id")?,
        model_route_rule_id: uuid_optional(row, "model_route_rule_id")?,
        mcp_server_id: uuid_optional(row, "mcp_server_id")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cached_tokens: row.try_get("cached_tokens")?,
        cache_read_tokens: row.try_get("cache_read_tokens")?,
        cache_write_tokens: row.try_get("cache_write_tokens")?,
        error_code: optional_string(row, "error_code")?,
        failure_family: optional_string(row, "failure_family")?,
        redaction_applied: required_bool(row, "redaction_applied")?,
        redaction_findings_count: i32::try_from(row.try_get::<i64, _>("redaction_findings_count")?)
            .map_err(|_| {
                StandaloneConfigError::CorruptDatabase(
                    "redaction_findings_count is outside i32 range".to_string(),
                )
            })?,
        redaction_replacements_count: i32::try_from(
            row.try_get::<i64, _>("redaction_replacements_count")?,
        )
        .map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "redaction_replacements_count is outside i32 range".to_string(),
            )
        })?,
        redaction_types: parse_string_list(&redaction_types_json, "redaction_types_json")?,
        redaction_fields: parse_string_list(&redaction_fields_json, "redaction_fields_json")?,
        route_selection_reason: required_string(row, "route_selection_reason")?,
        user_id: row.try_get("user_id")?,
        client_key_id: row.try_get("client_key_id")?,
        client_key_label: optional_string(row, "client_key_label")?,
        request_user_agent: optional_string(row, "request_user_agent")?,
        endpoint_key_label: optional_string(row, "endpoint_key_label")?,
        mcp_server_name: optional_string(row, "mcp_server_name")?,
        mcp_protocol_method: optional_string(row, "mcp_protocol_method")?,
        mcp_operation_name: optional_string(row, "mcp_operation_name")?,
        http_request_content_encoding: optional_string(row, "http_request_content_encoding")?,
        http_request_compressed: required_bool(row, "http_request_compressed")?,
        http_request_compressed_bytes: row.try_get("http_request_compressed_bytes")?,
        http_request_decompressed_bytes: row.try_get("http_request_decompressed_bytes")?,
        http_request_compression_ratio: row.try_get("http_request_compression_ratio")?,
        conversation_source: required_string(row, "conversation_source")?,
        client_installation_id: optional_string(row, "client_installation_id")?,
        provider_response_id: optional_string(row, "provider_response_id")?,
        provider_conversation_key: optional_string(row, "provider_conversation_key")?,
        request_storage_mode: required_string(row, "request_storage_mode")?,
        error_message: optional_string(row, "error_message")?,
        request_has_previous_response_id: required_bool(row, "request_has_previous_response_id")?,
        request_previous_response_id: optional_string(row, "request_previous_response_id")?,
        request_previous_response_parent_found: optional_bool(
            row,
            "request_previous_response_parent_found",
        )?,
        request_conversation_key: optional_string(row, "request_conversation_key")?,
        request_conversation_parent_found: optional_bool(row, "request_conversation_parent_found")?,
        upstream_redaction_enabled: required_bool(row, "upstream_redaction_enabled")?,
        response_capture_truncated: required_bool(row, "response_capture_truncated")?,
    })
}
