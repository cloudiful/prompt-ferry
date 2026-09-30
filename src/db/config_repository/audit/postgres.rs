//! PostgreSQL read/write for the configuration archive audit trail.

use anyhow::{Context, Result};

use super::{ConfigAuditPage, ConfigAuditRecord, RawConfigAuditRow, clamp_audit_page};
use crate::db::config_repository::PostgresConfigRepository;

impl PostgresConfigRepository {
    pub(super) async fn record_config_audit(&self, record: &ConfigAuditRecord) -> Result<i64> {
        let domains = serde_json::to_value(&record.domains).context("serialize audit domains")?;
        let audit_id = sqlx::query_file_scalar!(
            "src/sql/audit/insert_config_archive_audit.sql",
            record.actor_user_id,
            record.action.as_str(),
            record.backend_kind.as_str(),
            record.format_version.map(i32::from),
            record.archive_bytes,
            record.payload_fingerprint.as_deref(),
            record.success,
            record.error_code.as_deref(),
            record.error_message.as_deref(),
            domains,
        )
        .fetch_one(self.pool())
        .await?;
        Ok(audit_id)
    }

    pub(super) async fn list_config_audit(&self, first: i64, rows: i64) -> Result<ConfigAuditPage> {
        let total = sqlx::query_file_scalar!("src/sql/audit/count_config_archive_audit.sql")
            .fetch_one(self.pool())
            .await?;
        let (first, rows) = clamp_audit_page(first, rows);
        let records = sqlx::query_file_as!(
            RawConfigAuditRow,
            "src/sql/audit/list_config_archive_audit.sql",
            first,
            rows,
        )
        .fetch_all(self.pool())
        .await?;
        let entries = records
            .into_iter()
            .map(RawConfigAuditRow::into_entry)
            .collect::<Result<Vec<_>>>()?;
        Ok(ConfigAuditPage {
            total,
            first,
            rows,
            entries,
        })
    }
}
