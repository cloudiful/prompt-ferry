//! Unified configuration repository for the worker admin API.
//!
//! The repository exposes a single backend-dispatching API so handlers do not
//! have to choose between PostgreSQL and SQLite. The PostgreSQL backend keeps
//! the previously published SQL implementations; the SQLite backend reuses the
//! existing `StandaloneConfigStore` so encrypted secrets and snapshot
//! publication semantics stay consistent with the SQLite runtime.
//!
//! Each domain lives in its own sibling module:
//!   - `endpoints.rs` for provider endpoint CRUD and endpoint API keys
//!   - `model_routes.rs` for model route rules and route targets
//!   - `relays.rs` for managed relays and snapshot publication
//!   - `client_keys.rs` for per-user client keys (admin and self-service)
//!   - `settings.rs` for worker-level JSON settings
//!   - `capabilities.rs` for per-path capability gating

pub mod capabilities;
pub mod client_keys;
pub mod endpoints;
pub mod endpoints_map;
pub mod endpoints_sqlite;
pub mod mcp;
pub mod model_routes;
pub mod model_routes_map;
pub mod relays;
pub mod relays_map;
pub mod settings;

use sqlx::PgPool;
use std::sync::Arc;

use crate::{relay_secrets::RelaySecretManager, standalone_config::StandaloneConfigStore};

pub use capabilities::Capability;
pub use client_keys::{UnifiedClientKey, UnifiedClientKeyCreated};
pub use endpoints::{UnifiedEndpointApiKey, UnifiedEndpointPage, UnifiedProviderEndpoint};
pub use model_routes::{UnifiedModelRoute, UnifiedModelRoutePage, UnifiedModelRouteTarget};
pub use relays::{ManagedRelaySecrets, UnifiedManagedRelay, relay_secrets_for_state};
pub use settings::UnifiedSetting;

#[derive(Clone)]
pub enum ConfigRepository {
    Postgres(PostgresConfigRepository),
    Sqlite(SqliteConfigRepository),
}

impl ConfigRepository {
    pub fn postgres(pool: &PgPool) -> Self {
        Self::Postgres(PostgresConfigRepository { pool: pool.clone() })
    }

    pub fn sqlite(store: Arc<StandaloneConfigStore>, manager: RelaySecretManager) -> Self {
        Self::Sqlite(SqliteConfigRepository { store, manager })
    }

    pub fn is_sqlite(&self) -> bool {
        matches!(self, Self::Sqlite(_))
    }

    pub fn as_postgres(&self) -> Option<&PgPool> {
        match self {
            Self::Postgres(repo) => Some(&repo.pool),
            Self::Sqlite(_) => None,
        }
    }

    pub fn as_sqlite(&self) -> Option<&SqliteConfigRepository> {
        match self {
            Self::Sqlite(repo) => Some(repo),
            Self::Postgres(_) => None,
        }
    }

    pub fn standalone_store(&self) -> Option<&Arc<StandaloneConfigStore>> {
        match self {
            Self::Sqlite(repo) => Some(&repo.store),
            Self::Postgres(_) => None,
        }
    }

    pub fn standalone_secret_manager(&self) -> Option<&RelaySecretManager> {
        match self {
            Self::Sqlite(repo) => Some(&repo.manager),
            Self::Postgres(_) => None,
        }
    }

    /// Look up the capability surface; PostgreSQL supports everything while
    /// SQLite still rejects unsupported domains with per-path capability
    /// errors.
    pub fn supports_capability(&self, capability: Capability) -> bool {
        match self {
            Self::Postgres(_) => true,
            Self::Sqlite(_) => capability.sqlite_supported(),
        }
    }
}

#[derive(Clone)]
pub struct PostgresConfigRepository {
    pool: PgPool,
}

impl PostgresConfigRepository {
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[derive(Clone)]
pub struct SqliteConfigRepository {
    store: Arc<StandaloneConfigStore>,
    manager: RelaySecretManager,
}

impl SqliteConfigRepository {
    pub fn store(&self) -> &Arc<StandaloneConfigStore> {
        &self.store
    }

    pub fn manager(&self) -> &RelaySecretManager {
        &self.manager
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.pool()
    }

    pub async fn reload_snapshot(&self) -> anyhow::Result<()> {
        let _ = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_supported_listing_is_explicit() {
        // The router middleware uses `capability.sqlite_supported()` to decide
        // whether a path is allowed. SQLite supports the unified configuration
        // and MCP configuration/catalog surface, but not advanced ledgers.
        assert!(Capability::SnapshotPublication.sqlite_supported());
        assert!(Capability::Endpoints.sqlite_supported());
        assert!(Capability::ModelRoutes.sqlite_supported());
        assert!(Capability::Relays.sqlite_supported());
        assert!(Capability::ClientKeys.sqlite_supported());
        assert!(Capability::Settings.sqlite_supported());
        assert!(Capability::EndpointSetting.sqlite_supported());
        assert!(Capability::McpServers.sqlite_supported());
        assert!(Capability::McpCredentials.sqlite_supported());
        assert!(Capability::McpCatalog.sqlite_supported());
        assert!(!Capability::RequestRecords.sqlite_supported());
        assert!(!Capability::Approvals.sqlite_supported());
        assert!(!Capability::Billing.sqlite_supported());
        assert!(!Capability::McpQuota.sqlite_supported());
        assert!(!Capability::ConversationEndpointOverride.sqlite_supported());
        assert!(!Capability::AvailableModels.sqlite_supported());
    }
}

#[cfg(test)]
mod integration_tests;
