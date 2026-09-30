//! Types for the admin encrypted configuration export.
//!
//! The passphrase travels in the JSON request body (never the query string),
//! stays inside the request scope, and is never persisted. Its `Debug`
//! implementation is redacted so an accidental log cannot leak it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::config_repository::ConfigBackendKind;

/// Passphrase-encrypted export request.
#[derive(Deserialize, ToSchema)]
pub struct ConfigExportRequest {
    /// Passphrase sealing the archive. Never stored, never logged, never
    /// placed in a URL.
    #[schema(value_type = String, write_only = true)]
    pub passphrase: String,
}

impl std::fmt::Debug for ConfigExportRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigExportRequest")
            .field("passphrase", &"[redacted]")
            .finish()
    }
}

/// Non-secret metadata about the produced archive. Returned by the metadata
/// variant so an operator can record the package fingerprint before storing
/// the bytes.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigExportMetadata {
    #[schema(value_type = String, example = "postgres")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: u16,
    /// SHA-256 (lowercase hex) of the snapshot payload; identifies the
    /// package contents without revealing them.
    pub payload_fingerprint: String,
    pub domains: Vec<ConfigExportDomainSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigExportDomainSummary {
    pub name: String,
    pub records: usize,
    pub unrecoverable_secrets: usize,
}
