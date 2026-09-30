//! Portable, passphrase-encryptable configuration snapshot.
//!
//! The snapshot is the plaintext half of the encrypted configuration archive:
//! it describes every restorable configuration domain of one worker, tags the
//! storage backend it was read from, and keeps an explicit recoverable /
//! unrecoverable marker per secret so a future import can tell "the original
//! value is here" from "only a hash (or nothing) survived on the source".
//!
//! Runtime history is deliberately absent: request records, usage events,
//! billing charges, approvals, sessions, and caches never appear here.
//!
//! The record shapes live in [`domains`]; the manifest, its summary, and the
//! sealed payload wrapper live in [`manifest`].

mod domains;
mod manifest;

pub use domains::{
    ClientKeySnapshot, EndpointApiKeySnapshot, EndpointOAuthSnapshot, EndpointSnapshot,
    McpCredentialSnapshot, McpServerSnapshot, ModelRouteSnapshot, ModelRouteTargetSnapshot,
    RelaySnapshot, SecretRecovery, SettingSnapshot, SnapshotDomains, UserRedactionConfigSnapshot,
    UserSnapshot,
};
pub use manifest::{
    ConfigBackendKind, ConfigSnapshot, SNAPSHOT_FORMAT_VERSION, SnapshotDomainSummary,
    SnapshotManifest, hex_lower, payload_fingerprint,
};
