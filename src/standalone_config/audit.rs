//! SQLite persistence for the administrator configuration archive audit trail.
//!
//! The statements live under `src/standalone_config/sql/audit/` and are loaded
//! through the standalone text-query wrapper, because the crate-wide
//! `DATABASE_URL` points at PostgreSQL while these statements target the
//! runtime-selected standalone SQLite database.
//!
//! The trail stores non-secret metadata only; see
//! [`crate::db::config_repository::audit`] for the shared contract.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::Row;

use super::StandaloneConfigStore;
use crate::db::config_repository::audit::{
    ConfigAuditEntry, ConfigAuditPage, ConfigAuditRecord, RawConfigAuditRow, clamp_audit_page,
};

impl StandaloneConfigStore {
    pub(crate) async fn record_config_audit(&self, record: &ConfigAuditRecord) -> Result<i64> {
        let domains = serde_json::to_string(&record.domains).context("serialize audit domains")?;
        let outcome =
            standalone_query!("src/standalone_config/sql/audit/insert_config_archive_audit.sql")
                .bind(record.actor_user_id)
                .bind(record.action.as_str())
                .bind(record.backend_kind.as_str())
                .bind(record.format_version.map(i64::from))
                .bind(record.archive_bytes)
                .bind(record.payload_fingerprint.as_deref())
                .bind(record.success)
                .bind(record.error_code.as_deref())
                .bind(record.error_message.as_deref())
                .bind(domains)
                .bind(Utc::now().to_rfc3339())
                .execute(&self.pool)
                .await
                .context("failed to persist the configuration archive audit row")?;
        Ok(outcome.last_insert_rowid())
    }

    pub(crate) async fn list_config_audit(&self, first: i64, rows: i64) -> Result<ConfigAuditPage> {
        let total =
            standalone_query!("src/standalone_config/sql/audit/count_config_archive_audit.sql")
                .fetch_one(&self.pool)
                .await
                .context("failed to count the configuration archive audit rows")?
                .try_get::<i64, _>("total")?;
        let (first, rows) = clamp_audit_page(first, rows);
        let records =
            standalone_query!("src/standalone_config/sql/audit/list_config_archive_audit.sql")
                .bind(rows)
                .bind(first)
                .fetch_all(&self.pool)
                .await
                .context("failed to read the configuration archive audit page")?;
        let entries = records
            .into_iter()
            .map(audit_entry)
            .collect::<Result<Vec<_>>>()?;
        Ok(ConfigAuditPage {
            total,
            first,
            rows,
            entries,
        })
    }
}

fn audit_entry(row: sqlx::sqlite::SqliteRow) -> Result<ConfigAuditEntry> {
    let domain_summary: String = row.try_get("domain_summary")?;
    let created_at: String = row.try_get("created_at")?;
    RawConfigAuditRow {
        audit_id: row.try_get("audit_id")?,
        actor_user_id: row.try_get("actor_user_id")?,
        actor_login_name: row.try_get("actor_login_name")?,
        action: row.try_get("action")?,
        backend_kind: row.try_get("backend_kind")?,
        format_version: row
            .try_get::<Option<i64>, _>("format_version")?
            .map(|version| i32::try_from(version).unwrap_or_default()),
        archive_bytes: row.try_get("archive_bytes")?,
        payload_fingerprint: row.try_get("payload_fingerprint")?,
        success: row.try_get::<i64, _>("success")? != 0,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        domain_summary: serde_json::from_str(&domain_summary)
            .context("audit row carries an invalid domain summary")?,
        created_at: parse_audit_timestamp(&created_at)?,
    }
    .into_entry()
}

/// SQLite stores audit timestamps as RFC3339 text; the table default is the
/// second-precision `CURRENT_TIMESTAMP` form, which is also accepted so a
/// manually inserted row never breaks the read.
fn parse_audit_timestamp(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&Utc));
    }
    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .context("audit row carries an invalid created_at value")?;
    Ok(DateTime::from_naive_utc_and_offset(naive, Utc))
}
