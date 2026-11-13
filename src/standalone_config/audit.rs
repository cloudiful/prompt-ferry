//! SQLite persistence for the administrator configuration archive audit trail.
//!
//! The native entity for `standalone_config_archive_audit` owns the ordinary
//! insert and paged read; the columns stay the same as the applied migration
//! and the trail stores non-secret metadata only. See
//! [`crate::db::config_repository::audit`] for the shared contract.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query};
use sea_orm::{ConnectionTrait, EntityTrait, PaginatorTrait, Set};

use crate::db::entities::sqlite::standalone_config_archive_audit::{ActiveModel, Column, Entity};
use crate::db::seaorm::sqlite_connection;

use super::StandaloneConfigStore;
use crate::db::config_repository::audit::{
    ConfigAuditEntry, ConfigAuditPage, ConfigAuditRecord, RawConfigAuditRow, clamp_audit_page,
};

impl StandaloneConfigStore {
    pub(crate) async fn record_config_audit(&self, record: &ConfigAuditRecord) -> Result<i64> {
        let domains = serde_json::to_string(&record.domains).context("serialize audit domains")?;
        let db = sqlite_connection(&self.pool);
        let outcome = Entity::insert(ActiveModel {
            actor_user_id: Set(record.actor_user_id),
            action: Set(record.action.as_str().to_string()),
            backend_kind: Set(record.backend_kind.as_str().to_string()),
            format_version: Set(record.format_version.map(i32::from)),
            archive_bytes: Set(record.archive_bytes),
            payload_fingerprint: Set(record.payload_fingerprint.clone()),
            success: Set(i32::from(record.success)),
            error_code: Set(record.error_code.clone()),
            error_message: Set(record.error_message.clone()),
            domain_summary: Set(domains),
            created_at: Set(Utc::now().to_rfc3339()),
            ..Default::default()
        })
        .exec(&db)
        .await
        .context("failed to persist the configuration archive audit row")?;
        Ok(outcome.last_insert_id)
    }

    pub(crate) async fn list_config_audit(&self, first: i64, rows: i64) -> Result<ConfigAuditPage> {
        let db = sqlite_connection(&self.pool);
        let total = Entity::find()
            .count(&db)
            .await
            .context("failed to count the configuration archive audit rows")?
            as i64;
        let (first, rows) = clamp_audit_page(first, rows);
        let users = Alias::new("standalone_users");
        let query = Query::select()
            .column((Entity, Column::AuditId))
            .column((Entity, Column::ActorUserId))
            .expr_as(
                Expr::col((users.clone(), Alias::new("login_name"))),
                Alias::new("actor_login_name"),
            )
            .column((Entity, Column::Action))
            .column((Entity, Column::BackendKind))
            .column((Entity, Column::FormatVersion))
            .column((Entity, Column::ArchiveBytes))
            .column((Entity, Column::PayloadFingerprint))
            .column((Entity, Column::Success))
            .column((Entity, Column::ErrorCode))
            .column((Entity, Column::ErrorMessage))
            .column((Entity, Column::DomainSummary))
            .column((Entity, Column::CreatedAt))
            .from(Entity)
            .join(
                JoinType::LeftJoin,
                users.clone(),
                Expr::col((Entity, Column::ActorUserId))
                    .equals((users.clone(), Alias::new("user_id"))),
            )
            .order_by((Entity, Column::CreatedAt), Order::Desc)
            .order_by((Entity, Column::AuditId), Order::Desc)
            .limit(rows as u64)
            .offset(first as u64)
            .to_owned();
        let records = db
            .query_all(&query)
            .await
            .context("failed to read the configuration archive audit page")?;
        let entries = records
            .iter()
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

fn audit_entry(row: &sea_orm::QueryResult) -> Result<ConfigAuditEntry> {
    let domain_summary: String = row.try_get("", "domain_summary")?;
    let created_at: String = row.try_get("", "created_at")?;
    let format_version: Option<i64> = row.try_get("", "format_version")?;
    RawConfigAuditRow {
        audit_id: row.try_get("", "audit_id")?,
        actor_user_id: row.try_get("", "actor_user_id")?,
        actor_login_name: row.try_get("", "actor_login_name")?,
        action: row.try_get("", "action")?,
        backend_kind: row.try_get("", "backend_kind")?,
        format_version: format_version.map(|version| i32::try_from(version).unwrap_or_default()),
        archive_bytes: row.try_get("", "archive_bytes")?,
        payload_fingerprint: row.try_get("", "payload_fingerprint")?,
        success: row.try_get::<i64>("", "success")? != 0,
        error_code: row.try_get("", "error_code")?,
        error_message: row.try_get("", "error_message")?,
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
