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

    pub const fn supports_shared_workers(self) -> bool {
        matches!(self, Self::Valkey | Self::Postgres | Self::Sqlite)
    }

    pub const fn is_process_local(self) -> bool {
        matches!(self, Self::InMemory)
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

    #[test]
    fn sqlite_contract_is_durable_with_explicit_advanced_limitations() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, "");

        assert!(contract.capabilities.durable_configuration);
        assert!(contract.capabilities.encrypted_secrets);
        assert!(contract.capabilities.users);
        assert!(contract.capabilities.client_keys);
        assert!(contract.capabilities.mcp);
        assert!(!contract.capabilities.request_usage);
        assert!(!contract.capabilities.approvals);
        assert!(!contract.capabilities.billing);
        assert!(!contract.capabilities.shared_workers);
        assert_eq!(contract.coordinator, CoordinatorBackend::Sqlite);
        assert_eq!(contract.sessions, StateBackend::Sqlite);
        assert_eq!(contract.quota_ledger, StateBackend::Unavailable);
    }

    #[test]
    fn valkey_is_optional_and_explicitly_enables_shared_coordination() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, " redis://valkey ");

        assert_eq!(contract.coordinator, CoordinatorBackend::Valkey);
        assert!(contract.coordinator.supports_shared_workers());
        assert!(!contract.capabilities.shared_workers);
        assert_eq!(contract.maintenance, StateBackend::Valkey);
    }

    #[test]
    fn postgres_without_valkey_keeps_durable_state_on_postgres() {
        let contract = StorageContract::for_backend(StorageBackend::Postgres, "");

        assert_eq!(contract.coordinator, CoordinatorBackend::Postgres);
        assert_eq!(contract.replay, StateBackend::Postgres);
        assert_eq!(contract.sessions, StateBackend::Unavailable);
        assert_eq!(contract.quota_ledger, StateBackend::Postgres);
        assert_eq!(contract.maintenance, StateBackend::Postgres);
    }
}
