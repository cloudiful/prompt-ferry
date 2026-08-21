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
                users: false,
                client_keys: true,
                endpoints: true,
                routes: true,
                relays: true,
                settings: true,
                request_usage: false,
                raw_payload_retention: false,
                approvals: false,
                mcp: false,
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
    InMemory,
}

impl CoordinatorBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valkey => "valkey",
            Self::InMemory => "memory",
        }
    }

    pub const fn supports_shared_workers(self) -> bool {
        matches!(self, Self::Valkey)
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
}

impl StorageContract {
    pub fn for_backend(backend: StorageBackend, valkey_url: &str) -> Self {
        let coordinator = if valkey_url.trim().is_empty() {
            CoordinatorBackend::InMemory
        } else {
            CoordinatorBackend::Valkey
        };
        Self {
            backend,
            coordinator,
            capabilities: backend.capabilities(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoordinatorBackend, StorageBackend, StorageContract};

    #[test]
    fn sqlite_contract_is_durable_but_not_claimed_as_shared_or_user_complete() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, "");

        assert!(contract.capabilities.durable_configuration);
        assert!(contract.capabilities.encrypted_secrets);
        assert!(contract.capabilities.client_keys);
        assert!(!contract.capabilities.users);
        assert!(!contract.capabilities.shared_workers);
        assert_eq!(contract.coordinator, CoordinatorBackend::InMemory);
    }

    #[test]
    fn valkey_is_optional_and_explicitly_enables_shared_coordination() {
        let contract = StorageContract::for_backend(StorageBackend::Sqlite, " redis://valkey ");

        assert_eq!(contract.coordinator, CoordinatorBackend::Valkey);
        assert!(contract.coordinator.supports_shared_workers());
        assert!(!contract.capabilities.shared_workers);
    }
}
