//! Contracts shared by the worker's durable and coordination backends.
//!
//! The contract describes what a backend can safely provide today. It is
//! intentionally independent from the domain repositories, which can adopt
//! it incrementally without making PostgreSQL and SQLite look interchangeable
//! before their schemas and queries are compatible.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Postgres,
    Sqlite,
}

impl StorageBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }

    pub const fn is_postgres(self) -> bool {
        matches!(self, Self::Postgres)
    }

    pub const fn is_sqlite(self) -> bool {
        matches!(self, Self::Sqlite)
    }

    pub const fn capabilities(self) -> StorageCapabilities {
        match self {
            Self::Postgres => StorageCapabilities {
                backend: self,
                durable_configuration: true,
                encrypted_secrets: true,
                users: true,
                client_keys: true,
                endpoints: true,
                routes: true,
                relays: true,
                settings: true,
                request_usage: true,
                raw_payload_retention: true,
                approvals: true,
                mcp: true,
                billing: true,
                durable_replay: true,
                shared_workers: true,
            },
            Self::Sqlite => StorageCapabilities {
                backend: self,
                durable_configuration: true,
                encrypted_secrets: true,
                users: true,
                client_keys: true,
                endpoints: true,
                routes: true,
                relays: true,
                settings: true,
                request_usage: false,
                raw_payload_retention: false,
                approvals: false,
                mcp: true,
                billing: false,
                durable_replay: false,
                shared_workers: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorBackend {
    Valkey,
    Postgres,
    Sqlite,
    InMemory,
}

impl CoordinatorBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valkey => "valkey",
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
            Self::InMemory => "memory",
        }
    }

    /// Shared-worker coordination is supported when the coordinator can
    /// safely arbitrate between processes on different hosts. Valkey and
    /// PostgreSQL do that natively; SQLite keeps a durable coordinator
    /// table for single-host/lease coordination but is explicitly
    /// single-worker on one host.
    pub const fn supports_shared_workers(self) -> bool {
        matches!(self, Self::Valkey | Self::Postgres)
    }

    pub const fn is_process_local(self) -> bool {
        matches!(self, Self::InMemory)
    }

    /// True when the coordinator backend survives process restart. Valkey,
    /// PostgreSQL, and SQLite all qualify; only the in-memory fallback does
    /// not.
    pub const fn is_durable(self) -> bool {
        matches!(self, Self::Valkey | Self::Postgres | Self::Sqlite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateBackend {
    Valkey,
    Postgres,
    Sqlite,
    Memory,
    Unavailable,
}

impl StateBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valkey => "valkey",
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
            Self::Memory => "memory",
            Self::Unavailable => "unavailable",
        }
    }

    pub const fn is_process_local(self) -> bool {
        matches!(self, Self::Memory)
    }

    /// True when the state backend is durable across process restart.
    /// Unavailable is not considered durable because no data is held.
    pub const fn is_durable(self) -> bool {
        matches!(self, Self::Valkey | Self::Postgres | Self::Sqlite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageCapabilities {
    pub backend: StorageBackend,
    pub durable_configuration: bool,
    pub encrypted_secrets: bool,
    pub users: bool,
    pub client_keys: bool,
    pub endpoints: bool,
    pub routes: bool,
    pub relays: bool,
    pub settings: bool,
    pub request_usage: bool,
    pub raw_payload_retention: bool,
    pub approvals: bool,
    pub mcp: bool,
    pub billing: bool,
    pub durable_replay: bool,
    pub shared_workers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageContract {
    pub backend: StorageBackend,
    pub coordinator: CoordinatorBackend,
    pub capabilities: StorageCapabilities,
    pub replay: StateBackend,
    pub sessions: StateBackend,
    pub response_affinity: StateBackend,
    pub mcp_catalog_cache: StateBackend,
    pub mcp_session_cache: StateBackend,
    pub quota_ledger: StateBackend,
    pub maintenance: StateBackend,
}

impl StorageContract {
    pub fn for_backend(backend: StorageBackend, valkey_url: &str) -> Self {
        let coordinator = if !valkey_url.trim().is_empty() {
            CoordinatorBackend::Valkey
        } else if backend.is_sqlite() {
            CoordinatorBackend::Sqlite
        } else {
            CoordinatorBackend::Postgres
        };
        let cache_backend = match coordinator {
            CoordinatorBackend::Valkey => StateBackend::Valkey,
            CoordinatorBackend::Sqlite => StateBackend::Sqlite,
            CoordinatorBackend::Postgres => StateBackend::Memory,
            CoordinatorBackend::InMemory => StateBackend::Memory,
        };
        Self {
            backend,
            coordinator,
            capabilities: backend.capabilities(),
            replay: match coordinator {
                CoordinatorBackend::Postgres => StateBackend::Postgres,
                _ => cache_backend,
            },
            sessions: match coordinator {
                CoordinatorBackend::Postgres => StateBackend::Unavailable,
                _ => cache_backend,
            },
            response_affinity: cache_backend,
            mcp_catalog_cache: cache_backend,
            mcp_session_cache: cache_backend,
            quota_ledger: if backend.is_postgres() {
                StateBackend::Postgres
            } else {
                StateBackend::Unavailable
            },
            maintenance: match coordinator {
                CoordinatorBackend::Valkey => StateBackend::Valkey,
                CoordinatorBackend::Postgres => StateBackend::Postgres,
                CoordinatorBackend::Sqlite => StateBackend::Sqlite,
                CoordinatorBackend::InMemory => StateBackend::Unavailable,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoordinatorBackend, StateBackend, StorageBackend, StorageContract};

    fn assert_contract_invariants(contract: &StorageContract) {
        // quota_ledger availability must track the durable backend: only
        // SQLite is currently gated to Unavailable; PostgreSQL always owns
        // the quota ledger.
        if contract.backend.is_postgres() {
            assert_eq!(contract.quota_ledger, StateBackend::Postgres);
        } else {
            assert_eq!(
                contract.quota_ledger,
                StateBackend::Unavailable,
                "SQLite quota ledger must be explicitly unavailable until Phase 5",
            );
        }
        // Maintenance is unavailable only when the coordinator falls back to
        // the in-memory variant. All non-process-local coordinators own a
        // durable maintenance backend.
        if contract.coordinator == CoordinatorBackend::InMemory {
            assert_eq!(contract.maintenance, StateBackend::Unavailable);
        } else {
            assert!(
                contract.maintenance.is_durable(),
                "durable coordinator must own a durable maintenance backend",
            );
        }
        // Process-local state caches must never appear in slots that the
        // durable coordinator is expected to own. The PostgreSQL sessions
        // slot is the documented exception because Postgres has its own
        // session state and the contract marks it Unavailable rather than
        // memory-backed.
        if contract.coordinator != CoordinatorBackend::InMemory
            && !(contract.backend.is_postgres()
                && contract.coordinator == CoordinatorBackend::Postgres)
        {
            for state in [
                contract.replay,
                contract.sessions,
                contract.response_affinity,
                contract.mcp_catalog_cache,
                contract.mcp_session_cache,
            ] {
                assert!(
                    state.is_durable(),
                    "state slot must be durable when coordinator is non-process-local",
                );
            }
        }
    }

    #[test]
    fn sqlite_without_valkey_uses_sqlite_for_durable_state_and_single_worker_coordination() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, "");

        assert_eq!(contract.backend, StorageBackend::Sqlite);
        assert_eq!(contract.coordinator, CoordinatorBackend::Sqlite);
        assert!(contract.coordinator.is_durable());
        assert!(
            !contract.coordinator.supports_shared_workers(),
            "SQLite coordinator must not advertise shared-worker support",
        );

        assert_eq!(contract.replay, StateBackend::Sqlite);
        assert_eq!(contract.sessions, StateBackend::Sqlite);
        assert_eq!(contract.response_affinity, StateBackend::Sqlite);
        assert_eq!(contract.mcp_catalog_cache, StateBackend::Sqlite);
        assert_eq!(contract.mcp_session_cache, StateBackend::Sqlite);
        assert_eq!(contract.quota_ledger, StateBackend::Unavailable);
        assert_eq!(contract.maintenance, StateBackend::Sqlite);

        assert!(contract.capabilities.durable_configuration);
        assert!(contract.capabilities.encrypted_secrets);
        assert!(contract.capabilities.users);
        assert!(contract.capabilities.client_keys);
        assert!(contract.capabilities.endpoints);
        assert!(contract.capabilities.routes);
        assert!(contract.capabilities.relays);
        assert!(contract.capabilities.settings);
        assert!(contract.capabilities.mcp);
        assert!(!contract.capabilities.request_usage);
        assert!(!contract.capabilities.raw_payload_retention);
        assert!(!contract.capabilities.approvals);
        assert!(!contract.capabilities.billing);
        assert!(!contract.capabilities.durable_replay);
        assert!(
            !contract.capabilities.shared_workers,
            "SQLite storage backend must not advertise shared-worker support",
        );

        assert_contract_invariants(&contract);
    }

    #[test]
    fn sqlite_with_valkey_uses_valkey_for_coordination_but_keeps_sqlite_storage_single_worker() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, " redis://valkey ");

        assert_eq!(contract.backend, StorageBackend::Sqlite);
        assert_eq!(contract.coordinator, CoordinatorBackend::Valkey);
        assert!(contract.coordinator.supports_shared_workers());
        assert!(contract.coordinator.is_durable());

        assert_eq!(contract.replay, StateBackend::Valkey);
        assert_eq!(contract.sessions, StateBackend::Valkey);
        assert_eq!(contract.response_affinity, StateBackend::Valkey);
        assert_eq!(contract.mcp_catalog_cache, StateBackend::Valkey);
        assert_eq!(contract.mcp_session_cache, StateBackend::Valkey);
        assert_eq!(contract.quota_ledger, StateBackend::Unavailable);
        assert_eq!(contract.maintenance, StateBackend::Valkey);

        assert!(contract.capabilities.mcp);
        assert!(!contract.capabilities.request_usage);
        assert!(
            !contract.capabilities.shared_workers,
            "SQLite storage backend must remain single-host even when Valkey coordinates",
        );

        assert_contract_invariants(&contract);
    }

    #[test]
    fn postgres_without_valkey_keeps_durable_state_on_postgres() {
        let contract = StorageContract::for_backend(StorageBackend::Postgres, "");

        assert_eq!(contract.backend, StorageBackend::Postgres);
        assert_eq!(contract.coordinator, CoordinatorBackend::Postgres);
        assert!(contract.coordinator.is_durable());
        assert!(contract.coordinator.supports_shared_workers());

        assert_eq!(contract.replay, StateBackend::Postgres);
        assert_eq!(contract.sessions, StateBackend::Unavailable);
        assert_eq!(contract.response_affinity, StateBackend::Memory);
        assert_eq!(contract.mcp_catalog_cache, StateBackend::Memory);
        assert_eq!(contract.mcp_session_cache, StateBackend::Memory);
        assert_eq!(contract.quota_ledger, StateBackend::Postgres);
        assert_eq!(contract.maintenance, StateBackend::Postgres);

        assert!(contract.capabilities.request_usage);
        assert!(contract.capabilities.raw_payload_retention);
        assert!(contract.capabilities.approvals);
        assert!(contract.capabilities.billing);
        assert!(contract.capabilities.durable_replay);
        assert!(contract.capabilities.shared_workers);

        assert_contract_invariants(&contract);
    }

    #[test]
    fn postgres_with_valkey_routes_caches_through_valkey_and_keeps_postgres_for_durable_state() {
        let contract = StorageContract::for_backend(StorageBackend::Postgres, "redis://valkey");

        assert_eq!(contract.backend, StorageBackend::Postgres);
        assert_eq!(contract.coordinator, CoordinatorBackend::Valkey);
        assert!(contract.coordinator.is_durable());
        assert!(contract.coordinator.supports_shared_workers());

        assert_eq!(contract.replay, StateBackend::Valkey);
        assert_eq!(contract.sessions, StateBackend::Valkey);
        assert_eq!(contract.response_affinity, StateBackend::Valkey);
        assert_eq!(contract.mcp_catalog_cache, StateBackend::Valkey);
        assert_eq!(contract.mcp_session_cache, StateBackend::Valkey);
        assert_eq!(contract.quota_ledger, StateBackend::Postgres);
        assert_eq!(contract.maintenance, StateBackend::Valkey);

        assert!(contract.capabilities.shared_workers);
        assert!(contract.capabilities.billing);
        assert!(contract.capabilities.request_usage);

        assert_contract_invariants(&contract);
    }

    #[test]
    fn coordinator_and_storage_shared_worker_signals_are_consistent_for_sqlite() {
        // SQLite is the only backend whose coordinator must not advertise
        // shared-worker support. The storage capability flag must mirror
        // that signal so contract consumers do not see drift between the
        // coordinator description and the storage description.
        for valkey_url in ["", "redis://valkey"] {
            let contract = StorageContract::for_backend(StorageBackend::Sqlite, valkey_url);
            assert!(
                contract.coordinator.supports_shared_workers()
                    || !contract.capabilities.shared_workers,
                "coordinator and storage shared-worker signals drifted for SQLite (valkey_url={valkey_url:?})",
            );
            assert_eq!(
                contract.capabilities.shared_workers,
                contract.backend == StorageBackend::Postgres,
                "shared_workers capability must equal backend == Postgres",
            );
            assert_eq!(
                contract.capabilities.shared_workers, false,
                "SQLite must never advertise shared_workers",
            );
        }
    }
}
