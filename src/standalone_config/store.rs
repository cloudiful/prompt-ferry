use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use super::{Result, StandaloneConfigError};

// The administrator configuration archive audit trail lives in its own file so
// this store stays focused on configuration; it is a child module of `store`
// because it needs the private pool.
#[path = "audit.rs"]
mod audit;
#[path = "store_client_keys.rs"]
mod client_keys;
#[path = "store_endpoints.rs"]
mod endpoints;
#[path = "store_mcp.rs"]
mod mcp;
#[path = "store_mcp_oauth.rs"]
mod mcp_oauth;
#[path = "store_relays.rs"]
mod relays;
#[path = "store_replay_snapshots.rs"]
mod replay_snapshots;
#[path = "store_routes.rs"]
mod routes;
#[path = "store_settings.rs"]
mod settings;
#[path = "store_snapshot.rs"]
mod snapshot;
#[path = "store_usage_summaries.rs"]
mod usage_summaries;

const CURRENT_SCHEMA_VERSION: i64 = 39;

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
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            return Err(StandaloneConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "SQLite parent directory does not exist: {}",
                    parent.display()
                ),
            )));
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

    /// Share the underlying SQLite pool (integration-test support).
    ///
    /// Production code goes through the typed store methods; route-level
    /// integration tests under `tests/` build as an external crate and
    /// cannot see `pub(crate)` items, so they use this documented accessor
    /// to construct `UserStore::sqlite` and to close the pool on cleanup.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}
