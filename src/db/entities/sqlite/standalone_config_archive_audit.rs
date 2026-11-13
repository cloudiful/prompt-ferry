//! Native SeaORM entity for the standalone SQLite `standalone_config_archive_audit` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    Expr, ExprTrait, Index, IndexCreateStatement, IndexOrder, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `standalone_config_archive_audit(created_at DESC, audit_id DESC)` index.
pub const CREATED_AT_INDEX: &str = "idx_standalone_config_archive_audit_created_at";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_config_archive_audit")]
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
    #[sea_orm(column_type = "Integer")]
    pub success: i32,
    #[sea_orm(column_type = "Text")]
    pub error_code: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub error_message: Option<String>,
    #[sea_orm(column_type = "Text", default_value = "[]")]
    pub domain_summary: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_config_archive_audit`.
///
/// The applied action and success checks are table level constraints and are
/// declared natively; the applied foreign key to `standalone_users` stays
/// owned by the standalone migration contract, so no `Relation` is declared
/// here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_config_archive_audit_action_check",
        Expr::col(Column::Action).is_in(["export", "import"]),
    ));
    statement.check((
        "standalone_config_archive_audit_success_check",
        Expr::col(Column::Success).is_in([0, 1]),
    ));
    statement
}

/// Native index declarations for `standalone_config_archive_audit`.
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
