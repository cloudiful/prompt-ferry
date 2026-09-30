//! Administrator configuration import: dry-run preview and atomic replace.
//!
//! The import side of the encrypted configuration archive. `preview` decodes
//! and validates the archive against the running backend and reports
//! per-domain differences without writing anything; `apply` repeats the
//! validation and then replaces the restorable domains inside a single
//! backend transaction so a failure leaves no half-restored instance.
//!
//! Nothing here reads request/usage/billing/approval/session data, and no
//! secret, passphrase, or archive byte is ever logged, echoed, or placed in a
//! URL. The passphrase exists only inside the request scope.
//!
//! Backend readers/writers live in [`postgres`] and [`sqlite`]; this module
//! owns the portable types, validation, and preview diff.

mod diff;
mod postgres;
mod postgres_domains;
mod sqlite;
mod sqlite_rows;

use serde::Serialize;
use utoipa::ToSchema;

use super::archive::{ArchiveError, decode_archive_for_backend};
use super::snapshot::{ConfigBackendKind, ConfigSnapshot, SNAPSHOT_FORMAT_VERSION};
use super::{ConfigRepository, build_config_snapshot};
use crate::db::UserStore;
use crate::relay_secrets::RelaySecretManager;

/// The fixed set of restorable domains, in manifest order. Used to reject an
/// archive whose manifest declares an unknown or missing domain.
const DOMAIN_NAMES: [&str; 10] = [
    "users",
    "client_keys",
    "endpoints",
    "model_routes",
    "mcp_servers",
    "mcp_credentials",
    "relays",
    "settings",
    "user_redaction_configs",
    "raw_object_store",
];

/// Stable error surface for the import path. `code()` never carries archive
/// bytes, secrets, or the passphrase.
#[derive(Debug)]
pub enum ImportError {
    Archive(ArchiveError),
    /// The archive targets a domain the destination backend cannot store.
    UnsupportedDomain {
        domain: &'static str,
        backend: ConfigBackendKind,
    },
    /// The decoded payload is structurally unusable for import.
    InvalidArchive(&'static str),
    /// The request body or encoded archive exceeds the accepted size.
    TooLarge {
        limit: usize,
    },
    /// A destination write failed; the backend transaction is rolled back.
    Write(String),
}

impl ImportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Archive(err) => err.code(),
            Self::UnsupportedDomain { .. } => "config_import_unsupported_domain",
            Self::InvalidArchive(_) => "config_import_invalid_archive",
            Self::TooLarge { .. } => "config_import_too_large",
            Self::Write(_) => "config_import_write_failed",
        }
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(err) => write!(formatter, "{err}"),
            Self::UnsupportedDomain { domain, backend } => write!(
                formatter,
                "the {backend} backend cannot store the {domain} configuration domain"
            ),
            Self::InvalidArchive(reason) => {
                write!(
                    formatter,
                    "configuration archive cannot be imported: {reason}"
                )
            }
            Self::TooLarge { limit } => {
                write!(
                    formatter,
                    "configuration archive import request exceeds the {limit} byte limit"
                )
            }
            Self::Write(reason) => write!(formatter, "configuration import failed: {reason}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<ArchiveError> for ImportError {
    fn from(err: ArchiveError) -> Self {
        Self::Archive(err)
    }
}

/// Per-domain dry-run difference. Counts and identifiers only; never payloads.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportDomainDiff {
    pub name: String,
    pub archive_records: usize,
    pub target_records: usize,
    pub creates: usize,
    pub updates: usize,
    pub deletes: usize,
    pub unrecoverable_secrets: usize,
}

/// Non-secret preview of what an import would change.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportPreview {
    #[schema(value_type = String, example = "sqlite")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: u16,
    #[schema(value_type = String)]
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub payload_fingerprint: String,
    pub domains: Vec<ConfigImportDomainDiff>,
    pub warnings: Vec<String>,
}

/// One domain restored by a committed import.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportDomainSummary {
    pub name: String,
    pub records: usize,
    pub unrecoverable_secrets: usize,
}

/// Result of a committed import.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportApplied {
    #[schema(value_type = String, example = "sqlite")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: u16,
    pub payload_fingerprint: String,
    pub domains: Vec<ConfigImportDomainSummary>,
}

/// Decode and validate the archive against the running backend. Read-only.
pub fn decode_for_repository(
    repository: &ConfigRepository,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigSnapshot, ImportError> {
    let expected = if repository.is_sqlite() {
        ConfigBackendKind::Sqlite
    } else {
        ConfigBackendKind::Postgres
    };
    let snapshot = decode_archive_for_backend(passphrase, archive_bytes, Some(expected))?;
    validate_snapshot(&snapshot, expected)?;
    Ok(snapshot)
}

/// Reject archives whose manifest declares a shape the importer does not know,
/// or whose domains the destination backend cannot store.
fn validate_snapshot(
    snapshot: &ConfigSnapshot,
    backend: ConfigBackendKind,
) -> Result<(), ImportError> {
    if snapshot.manifest.format_version != SNAPSHOT_FORMAT_VERSION {
        return Err(ImportError::InvalidArchive("unsupported format version"));
    }
    let names: Vec<&str> = snapshot
        .manifest
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    if names.len() != DOMAIN_NAMES.len() || !DOMAIN_NAMES.iter().all(|name| names.contains(name)) {
        return Err(ImportError::InvalidArchive("unexpected snapshot domains"));
    }
    if backend == ConfigBackendKind::Sqlite {
        // SQLite stores neither per-user redaction rules nor the raw
        // object-store configuration; a payload carrying them is a foreign
        // shape and must be rejected instead of silently dropped.
        if !snapshot.domains.user_redaction_configs.is_empty() {
            return Err(ImportError::UnsupportedDomain {
                domain: "user_redaction_configs",
                backend,
            });
        }
        if snapshot.domains.raw_object_store.is_some() {
            return Err(ImportError::UnsupportedDomain {
                domain: "raw_object_store",
                backend,
            });
        }
    }
    Ok(())
}

/// Read-only dry run: decode the archive, compare it with the running
/// configuration, and return the per-domain differences plus warnings.
pub async fn preview_config_import(
    repository: &ConfigRepository,
    users: &UserStore,
    manager: Option<&RelaySecretManager>,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigImportPreview, ImportError> {
    let archive = decode_for_repository(repository, passphrase, archive_bytes)?;
    let target = build_config_snapshot(repository, users, manager)
        .await
        .map_err(|err| {
            ImportError::Write(format!("failed to read the current configuration: {err}"))
        })?;
    let domains = diff::diff_domains(&archive.domains, &target.domains);
    let warnings = diff::import_warnings(&archive);
    Ok(ConfigImportPreview {
        backend_kind: archive.manifest.backend_kind,
        format_version: archive.manifest.format_version,
        exported_at: archive.manifest.exported_at,
        payload_fingerprint: archive.manifest.payload_fingerprint.clone(),
        domains,
        warnings,
    })
}

/// Replace the restorable configuration domains with the archive contents in
/// one backend transaction.
pub async fn apply_config_import(
    repository: &ConfigRepository,
    manager: Option<&RelaySecretManager>,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigImportApplied, ImportError> {
    let archive = decode_for_repository(repository, passphrase, archive_bytes)?;
    match repository {
        ConfigRepository::Postgres(repo) => {
            let import_manager = manager.ok_or(ImportError::InvalidArchive(
                "the worker has no configuration encryption key for relay secrets",
            ))?;
            postgres::apply(repo.pool(), import_manager, &archive.domains)
                .await
                .map_err(|err| ImportError::Write(format!("{err:#}")))?;
        }
        ConfigRepository::Sqlite(repo) => {
            sqlite::apply(repo.store(), repo.manager(), &archive.domains)
                .await
                .map_err(|err| ImportError::Write(format!("{err:#}")))?;
        }
    }
    Ok(ConfigImportApplied {
        backend_kind: archive.manifest.backend_kind,
        format_version: archive.manifest.format_version,
        payload_fingerprint: archive.manifest.payload_fingerprint.clone(),
        domains: archive
            .manifest
            .domains
            .iter()
            .map(|domain| ConfigImportDomainSummary {
                name: domain.name.clone(),
                records: domain.records,
                unrecoverable_secrets: domain.unrecoverable_secrets,
            })
            .collect(),
    })
}
