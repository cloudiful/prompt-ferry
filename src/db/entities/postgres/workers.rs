//! Native SeaORM entity for the PostgreSQL `workers` registry.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "workers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub worker_id: String,
    #[sea_orm(column_type = "Text")]
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub version: String,
    pub incarnation: Uuid,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub started_at: DateTimeUtc,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub last_heartbeat_at: DateTimeUtc,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub stopped_at: Option<DateTimeUtc>,
    pub config_applied_revision: Option<i64>,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub config_restart_required: bool,
    #[sea_orm(column_type = "Text", default_expr = "Expr::cust(\"'bootstrap'\")")]
    pub coordination_mode: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `workers`.
pub fn create_table_statement() -> TableCreateStatement {
    Schema::new(DbBackend::Postgres).create_table_from_entity(Entity)
}
