//! PostgreSQL read/write for the configuration archive audit trail.

use anyhow::{Context, Result};
use sea_orm::Set;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query};
use sea_orm::{ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait};

use super::{ConfigAuditPage, ConfigAuditRecord, RawConfigAuditRow, clamp_audit_page};
use crate::db::config_repository::PostgresConfigRepository;
use crate::db::entities::postgres::config_archive_audit;
use crate::db::seaorm::{native_error, postgres_connection};

impl PostgresConfigRepository {
    pub(super) async fn record_config_audit(&self, record: &ConfigAuditRecord) -> Result<i64> {
        let domains = serde_json::to_value(&record.domains).context("serialize audit domains")?;
        let db = postgres_connection(self.pool());
        let row = config_archive_audit::Entity::insert(config_archive_audit::ActiveModel {
            actor_user_id: Set(record.actor_user_id),
            action: Set(record.action.as_str().to_string()),
            backend_kind: Set(record.backend_kind.as_str().to_string()),
            format_version: Set(record.format_version.map(i32::from)),
            archive_bytes: Set(record.archive_bytes),
            payload_fingerprint: Set(record.payload_fingerprint.clone()),
            success: Set(record.success),
            error_code: Set(record.error_code.clone()),
            error_message: Set(record.error_message.clone()),
            domain_summary: Set(domains),
            ..Default::default()
        })
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
        Ok(row.audit_id)
    }

    pub(super) async fn list_config_audit(&self, first: i64, rows: i64) -> Result<ConfigAuditPage> {
        let db = postgres_connection(self.pool());
        let total = config_archive_audit::Entity::find()
            .count(&db)
            .await
            .map_err(native_error)? as i64;
        let (first, rows) = clamp_audit_page(first, rows);
        let users = Alias::new("users");
        let query = Query::select()
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::AuditId,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::ActorUserId,
            ))
            .expr_as(
                Expr::col((users.clone(), Alias::new("login_name"))),
                Alias::new("actor_login_name"),
            )
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::Action,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::BackendKind,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::FormatVersion,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::ArchiveBytes,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::PayloadFingerprint,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::Success,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::ErrorCode,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::ErrorMessage,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::DomainSummary,
            ))
            .column((
                config_archive_audit::Entity,
                config_archive_audit::Column::CreatedAt,
            ))
            .from(config_archive_audit::Entity)
            .join(
                JoinType::LeftJoin,
                users.clone(),
                Expr::col((
                    config_archive_audit::Entity,
                    config_archive_audit::Column::ActorUserId,
                ))
                .equals((users.clone(), Alias::new("user_id"))),
            )
            .order_by(
                (
                    config_archive_audit::Entity,
                    config_archive_audit::Column::CreatedAt,
                ),
                Order::Desc,
            )
            .order_by(
                (
                    config_archive_audit::Entity,
                    config_archive_audit::Column::AuditId,
                ),
                Order::Desc,
            )
            .limit(rows as u64)
            .offset(first as u64)
            .to_owned();
        let query_results = db.query_all(&query).await.map_err(native_error)?;
        let entries = query_results
            .iter()
            .map(|row| RawConfigAuditRow::from_query_result(row, ""))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(native_error)?
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
