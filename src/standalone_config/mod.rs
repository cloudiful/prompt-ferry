macro_rules! standalone_query {
    ($path:literal) => {
        // SQLx query-file macros describe against the crate-wide DATABASE_URL. This
        // crate normally points that URL at PostgreSQL, while these statements target
        // the runtime-selected standalone SQLite database.
        sqlx::query(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/",
            $path
        )))
    };
}

mod coordinator;
mod models;
mod request_leases;
mod rows;
mod store;
#[cfg(test)]
mod tests;
mod users;
mod validation;
mod write;

pub(crate) use coordinator::StandaloneCoordinatorStore;
pub use models::{
    BootstrapSeed, ClientKeyConfig, ContinuationPolicy, EndpointApiKeyConfig, EndpointProvider,
    EndpointRegion, ManagedRelayConfig, MinimaxServiceTier, ModelRouteConfig,
    ModelRouteTargetConfig, ProviderEndpointConfig, ReplaySnapshotUpsertOutcome, Result,
    RouteScope, RoutingStrategy, SettingConfig, StandaloneConfig, StandaloneConfigError,
    StandaloneReplaySnapshotRecord, StandaloneUsageSummaryRecord,
};
pub(crate) use request_leases::{RequestLeaseAcquireOutcome, StandaloneRequestLeaseStore};
pub use store::{BootstrapOutcome, StandaloneConfigStore};
pub use users::SqliteUserStore;
