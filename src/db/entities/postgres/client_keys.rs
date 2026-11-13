//! Native SeaORM entity for the PostgreSQL `client_keys` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, Index, IndexCreateStatement, IndexOrder, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `client_keys(user_id, enabled, created_at DESC)` index.
pub const USER_INDEX: &str = "idx_client_keys_user";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "client_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub key_id: i64,
    #[sea_orm(column_type = "BigInteger")]
    pub user_id: i64,
    #[sea_orm(column_type = "Text")]
    pub key_prefix: String,
    #[sea_orm(column_type = "Text", unique)]
    pub key_hash: String,
    #[sea_orm(column_type = "Text")]
    pub label: String,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub enabled: bool,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub last_used_at: Option<DateTimeUtc>,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub created_at: DateTimeUtc,
    #[sea_orm(column_type = "Text")]
    pub secret: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `client_keys`.
///
/// The applied `key_id` is a `BIGSERIAL`, `key_hash` carries a uniqueness
/// constraint and `user_id` references `users`; the uniqueness is declared
/// natively and the foreign key stays owned by the migration contract, so no
/// `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    Schema::new(DbBackend::Postgres).create_table_from_entity(Entity)
}

/// Native index declarations for `client_keys`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(USER_INDEX)
            .table(Entity)
            .col(Column::UserId)
            .col(Column::Enabled)
            .col((Column::CreatedAt, IndexOrder::Desc))
            .to_owned(),
    ]
}
