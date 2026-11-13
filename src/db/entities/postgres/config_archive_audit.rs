//! Native SeaORM entity for the PostgreSQL `config_archive_audit` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    Expr, ExprTrait, Index, IndexCreateStatement, IndexOrder, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `config_archive_audit(created_at DESC, audit_id DESC)` index.
pub const CREATED_AT_INDEX: &str = "idx_config_archive_audit_created_at";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "config_archive_audit")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub audit_id: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub actor_user_id: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub action: String,
    #[sea_orm(column_type = "Text")]
    pub backend_kind: String,
    #[sea_orm(column_type = "Integer")]
    pub format_version: Option<i32>,
    #[sea_orm(column_type = "BigInteger")]
    pub archive_bytes: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub payload_fingerprint: Option<String>,
    #[sea_orm(column_type = "Boolean")]
    pub success: bool,
    #[sea_orm(column_type = "Text")]
    pub error_code: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub error_message: Option<String>,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub domain_summary: Json,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `config_archive_audit`.
///
/// The applied action check is a table level constraint and is declared
/// natively; the applied foreign key to `users` stays owned by the migration
/// contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "config_archive_audit_action_check",
        Expr::col(Column::Action).is_in(["export", "import"]),
    ));
    statement
}

/// Native index declarations for `config_archive_audit`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(CREATED_AT_INDEX)
            .table(Entity)
            .col((Column::CreatedAt, IndexOrder::Desc))
            .col((Column::AuditId, IndexOrder::Desc))
            .to_owned(),
    ]
}
