//! Manifest, payload wrapper, and fingerprint for the configuration snapshot.
//!
//! [`ConfigSnapshot`] is what the archive codec serializes: the manifest
//! describes the payload it travels with, and the fingerprint is a SHA-256 over
//! the canonical JSON of the domain records so a decoded snapshot can prove it
//! was not reshaped.

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::domains::{SecretRecovery, SnapshotDomains};

/// Version of the snapshot/manifest payload shape. Bumped whenever a domain
/// record gains an incompatible meaning so old archives are rejected instead
/// of being misread.
pub const SNAPSHOT_FORMAT_VERSION: u16 = 1;

/// Storage backend a snapshot was read from. Import only accepts a matching
/// backend so backend-specific secret wrapping is never silently destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigBackendKind {
    Postgres,
    Sqlite,
}

impl ConfigBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "postgres" => Some(Self::Postgres),
            "sqlite" => Some(Self::Sqlite),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConfigBackendKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub format_version: u16,
    pub backend_kind: ConfigBackendKind,
    pub exported_at: DateTime<Utc>,
    /// SHA-256 (lowercase hex) over the canonical JSON of the snapshot
    /// domains. Identifies the package contents without exposing them.
    pub payload_fingerprint: String,
    pub domains: Vec<SnapshotDomainSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDomainSummary {
    pub name: String,
    pub records: usize,
    /// Records inside this domain whose secret could not be recovered.
    pub unrecoverable_secrets: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub manifest: SnapshotManifest,
    #[serde(flatten)]
    pub domains: SnapshotDomains,
}

impl ConfigSnapshot {
    pub fn new(backend_kind: ConfigBackendKind, domains: SnapshotDomains) -> Self {
        Self {
            manifest: SnapshotManifest {
                format_version: SNAPSHOT_FORMAT_VERSION,
                backend_kind,
                exported_at: Utc::now(),
                payload_fingerprint: String::new(),
                domains: Vec::new(),
            },
            domains,
        }
    }

    /// Refresh the manifest summaries and fingerprint from the current domain
    /// records. Called by the archive encoder right before compression so the
    /// manifest always describes the bytes that are actually sealed.
    pub fn seal(&mut self) -> Result<()> {
        self.manifest.format_version = SNAPSHOT_FORMAT_VERSION;
        self.manifest.domains = self.domain_summaries();
        self.manifest.payload_fingerprint = self.compute_fingerprint()?;
        Ok(())
    }

    /// SHA-256 (lowercase hex) over the canonical JSON of the domain records.
    pub fn compute_fingerprint(&self) -> Result<String> {
        payload_fingerprint(&self.domains)
    }

    pub fn domain_summaries(&self) -> Vec<SnapshotDomainSummary> {
        vec![
            SnapshotDomainSummary {
                name: "users".to_string(),
                records: self.domains.users.len(),
                unrecoverable_secrets: self
                    .domains
                    .users
                    .iter()
                    .filter(|user| user.password_hash.is_none())
                    .count(),
            },
            SnapshotDomainSummary {
                name: "client_keys".to_string(),
                records: self.domains.client_keys.len(),
                unrecoverable_secrets: self
                    .domains
                    .client_keys
                    .iter()
                    .filter(|key| key.secret_state != SecretRecovery::Recovered)
                    .count(),
            },
            SnapshotDomainSummary {
                name: "endpoints".to_string(),
                records: self.domains.endpoints.len(),
                unrecoverable_secrets: self
                    .domains
                    .endpoints
                    .iter()
                    .filter(|endpoint| {
                        endpoint.api_key.is_none()
                            && endpoint.api_keys.iter().all(|key| key.api_key.is_none())
                    })
                    .count(),
            },
            SnapshotDomainSummary {
                name: "model_routes".to_string(),
                records: self.domains.model_routes.len(),
                unrecoverable_secrets: 0,
            },
            SnapshotDomainSummary {
                name: "mcp_servers".to_string(),
                records: self.domains.mcp_servers.len(),
                unrecoverable_secrets: 0,
            },
            SnapshotDomainSummary {
                name: "mcp_credentials".to_string(),
                records: self.domains.mcp_credentials.len(),
                unrecoverable_secrets: 0,
            },
            SnapshotDomainSummary {
                name: "relays".to_string(),
                records: self.domains.relays.len(),
                unrecoverable_secrets: self
                    .domains
                    .relays
                    .iter()
                    .filter(|relay| {
                        relay.tls_mode != "off" && relay.client_key_pem.is_none()
                            || relay.bridge_encryption_mode == "required"
                                && relay.bridge_encryption_key.is_none()
                    })
                    .count(),
            },
            SnapshotDomainSummary {
                name: "settings".to_string(),
                records: self.domains.settings.len(),
                unrecoverable_secrets: 0,
            },
            SnapshotDomainSummary {
                name: "user_redaction_configs".to_string(),
                records: self.domains.user_redaction_configs.len(),
                unrecoverable_secrets: 0,
            },
            SnapshotDomainSummary {
                name: "raw_object_store".to_string(),
                records: usize::from(self.domains.raw_object_store.is_some()),
                unrecoverable_secrets: 0,
            },
        ]
    }

    /// Reject snapshots whose manifest disagrees with the payload it came
    /// from. Cheap sanity pass run right after decoding.
    pub fn validate(&self) -> Result<()> {
        if self.manifest.format_version != SNAPSHOT_FORMAT_VERSION {
            bail!(
                "unsupported config snapshot format version {}",
                self.manifest.format_version
            );
        }
        let expected = self.compute_fingerprint()?;
        if self.manifest.payload_fingerprint != expected {
            bail!("config snapshot payload fingerprint mismatch");
        }
        Ok(())
    }
}

/// Canonical-JSON fingerprint of the snapshot domain records, shared by the
/// manifest builder and the archive decoder so the value is reproducible.
pub fn payload_fingerprint(domains: &SnapshotDomains) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(domains)?;
    Ok(hex_lower(&Sha256::digest(&bytes)))
}

/// Lowercase hex encoding shared by the snapshot manifest and archive header.
pub fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
