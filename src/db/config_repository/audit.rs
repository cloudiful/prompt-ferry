//! Administrator configuration archive audit trail.
//!
//! One row per real export/import attempt. The read-only import preview writes
//! nothing, and an attempt that never reached the point of producing or
//! applying an archive (missing or rejected admin session) is not audited.
//!
//! Only non-secret metadata is persisted: the operator id, the action, the
//! backend, the archive size, the payload fingerprint, the outcome, a stable
//! error code, a redacted bounded error summary, and the per-domain record
//! counts. The passphrase, the archive bytes, secret values, and raw
//! persistence errors never reach the table, and every write is best-effort so
//! a failing audit can never fail the export or import it describes.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::ConfigRepository;
use super::import::ConfigImportDomainSummary;
use super::snapshot::{ConfigBackendKind, SNAPSHOT_FORMAT_VERSION, SnapshotDomainSummary};
use crate::redact;

mod postgres;
#[cfg(test)]
mod tests;

/// Longest error summary persisted. The message is redacted and truncated
/// before the write, so a raw persistence error cannot land in the trail.
pub const MAX_AUDIT_ERROR_CHARS: usize = 200;

/// Largest audit page a caller may request; larger requests are clamped so a
/// single read cannot page in the whole trail.
pub const MAX_AUDIT_PAGE_ROWS: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAuditAction {
    Export,
    Import,
}

impl ConfigAuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "export" => Some(Self::Export),
            "import" => Some(Self::Import),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConfigAuditAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One domain's non-secret record counts inside an audited archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConfigAuditDomainCount {
    pub name: String,
    pub records: usize,
    pub unrecoverable_secrets: usize,
}

impl From<&SnapshotDomainSummary> for ConfigAuditDomainCount {
    fn from(summary: &SnapshotDomainSummary) -> Self {
        Self {
            name: summary.name.clone(),
            records: summary.records,
            unrecoverable_secrets: summary.unrecoverable_secrets,
        }
    }
}

impl From<&ConfigImportDomainSummary> for ConfigAuditDomainCount {
    fn from(summary: &ConfigImportDomainSummary) -> Self {
        Self {
            name: summary.name.clone(),
            records: summary.records,
            unrecoverable_secrets: summary.unrecoverable_secrets,
        }
    }
}

/// A pending audit row. Built by the handlers, which know the outcome and the
/// actor; the archive payload itself is never carried.
#[derive(Debug, Clone)]
pub struct ConfigAuditRecord {
    pub actor_user_id: Option<i64>,
    pub action: ConfigAuditAction,
    pub backend_kind: ConfigBackendKind,
    pub format_version: Option<u16>,
    pub archive_bytes: Option<i64>,
    pub payload_fingerprint: Option<String>,
    pub success: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub domains: Vec<ConfigAuditDomainCount>,
}

impl ConfigAuditRecord {
    /// A completed archive operation: the envelope was produced or applied.
    pub fn success(
        actor_user_id: Option<i64>,
        action: ConfigAuditAction,
        backend_kind: ConfigBackendKind,
        archive_bytes: i64,
        payload_fingerprint: &str,
        domains: Vec<ConfigAuditDomainCount>,
    ) -> Self {
        Self {
            actor_user_id,
            action,
            backend_kind,
            format_version: Some(SNAPSHOT_FORMAT_VERSION),
            archive_bytes: Some(archive_bytes),
            payload_fingerprint: Some(payload_fingerprint.to_string()),
            success: true,
            error_code: None,
            error_message: None,
            domains,
        }
    }

    /// A rejected or failed attempt. The message is redacted and bounded here
    /// so no caller can smuggle a secret or a raw persistence error into the
    /// trail.
    pub fn failure(
        actor_user_id: Option<i64>,
        action: ConfigAuditAction,
        backend_kind: ConfigBackendKind,
        archive_bytes: Option<i64>,
        error_code: &str,
        error_message: &str,
    ) -> Self {
        Self {
            actor_user_id,
            action,
            backend_kind,
            format_version: None,
            archive_bytes,
            payload_fingerprint: None,
            success: false,
            error_code: Some(error_code.to_string()),
            error_message: Some(sanitize_error_message(error_message)),
            domains: Vec::new(),
        }
    }
}

/// Redact and bound an error summary before it is persisted. The cap includes
/// the ellipsis, so a stored summary never exceeds [`MAX_AUDIT_ERROR_CHARS`].
pub fn sanitize_error_message(message: &str) -> String {
    const ELLIPSIS: &str = "...";
    let redacted = redact::redact_text(message);
    let mut bounded: String = redacted.chars().take(MAX_AUDIT_ERROR_CHARS).collect();
    if bounded.chars().count() < redacted.chars().count() {
        bounded = redacted
            .chars()
            .take(MAX_AUDIT_ERROR_CHARS - ELLIPSIS.len())
            .collect();
        bounded.push_str(ELLIPSIS);
    }
    bounded
}

/// One audited attempt as returned by the admin list endpoint.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigAuditEntry {
    pub audit_id: i64,
    pub actor_user_id: Option<i64>,
    /// Login of the operator, resolved from `actor_user_id`. `None` once the
    /// account was deleted; the audit row itself survives.
    pub actor_login_name: Option<String>,
    pub action: ConfigAuditAction,
    #[schema(value_type = String, example = "sqlite")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: Option<u16>,
    pub archive_bytes: Option<i64>,
    pub payload_fingerprint: Option<String>,
    pub success: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub domains: Vec<ConfigAuditDomainCount>,
    pub created_at: DateTime<Utc>,
}

/// One page of the audit trail, newest first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigAuditPage {
    pub total: i64,
    pub first: i64,
    pub rows: i64,
    pub entries: Vec<ConfigAuditEntry>,
}

/// Backend-neutral audit row, produced by the PostgreSQL decoder and by the
/// SQLite reader, then converted into the public entry shape.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawConfigAuditRow {
    pub audit_id: i64,
    pub actor_user_id: Option<i64>,
    pub actor_login_name: Option<String>,
    pub action: String,
    pub backend_kind: String,
    pub format_version: Option<i32>,
    pub archive_bytes: Option<i64>,
    pub payload_fingerprint: Option<String>,
    pub success: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub domain_summary: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl RawConfigAuditRow {
    pub(crate) fn into_entry(self) -> Result<ConfigAuditEntry> {
        Ok(ConfigAuditEntry {
            audit_id: self.audit_id,
            actor_user_id: self.actor_user_id,
            actor_login_name: self.actor_login_name,
            action: parse_action(&self.action)?,
            backend_kind: parse_backend(&self.backend_kind)?,
            format_version: self
                .format_version
                .and_then(|version| u16::try_from(version).ok()),
            archive_bytes: self.archive_bytes,
            payload_fingerprint: self.payload_fingerprint,
            success: self.success,
            error_code: self.error_code,
            error_message: self.error_message,
            domains: parse_domains(self.domain_summary)?,
            created_at: self.created_at,
        })
    }
}

impl ConfigRepository {
    /// Persist one audit row on the active backend. Best-effort by contract:
    /// callers decide whether a failure is fatal, and it never is.
    pub async fn record_config_audit(&self, record: &ConfigAuditRecord) -> Result<i64> {
        match self {
            Self::Postgres(repo) => repo.record_config_audit(record).await,
            Self::Sqlite(repo) => repo.store().record_config_audit(record).await,
        }
    }

    /// Read one page of the audit trail, newest first.
    pub async fn list_config_audit(&self, first: i64, rows: i64) -> Result<ConfigAuditPage> {
        match self {
            Self::Postgres(repo) => repo.list_config_audit(first, rows).await,
            Self::Sqlite(repo) => repo.store().list_config_audit(first, rows).await,
        }
    }
}

/// Clamp a requested page to the accepted window.
pub(crate) fn clamp_audit_page(first: i64, rows: i64) -> (i64, i64) {
    (first.max(0), rows.clamp(1, MAX_AUDIT_PAGE_ROWS))
}

fn parse_action(value: &str) -> Result<ConfigAuditAction> {
    ConfigAuditAction::parse(value)
        .with_context(|| format!("audit row carries an unknown action {value:?}"))
}

fn parse_backend(value: &str) -> Result<ConfigBackendKind> {
    ConfigBackendKind::parse(value)
        .with_context(|| format!("audit row carries an unknown backend {value:?}"))
}

fn parse_domains(value: serde_json::Value) -> Result<Vec<ConfigAuditDomainCount>> {
    serde_json::from_value(value).context("audit row carries an invalid domain summary")
}

/// The audit backend kind of the running repository.
pub fn repository_backend_kind(repository: &ConfigRepository) -> ConfigBackendKind {
    if repository.is_sqlite() {
        ConfigBackendKind::Sqlite
    } else {
        ConfigBackendKind::Postgres
    }
}

/// Persist an audit row best-effort. A failing audit never fails the
/// export/import it describes, so the error is logged without its payload.
pub async fn record_best_effort(repository: &ConfigRepository, record: ConfigAuditRecord) {
    if let Err(error) = repository.record_config_audit(&record).await {
        tracing::warn!(
            action = record.action.as_str(),
            success = record.success,
            "failed to persist configuration archive audit row: {error:#}"
        );
    }
}
