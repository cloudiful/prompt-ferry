//! Native SeaORM entity for the PostgreSQL `prompt_ferry_config_revision` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(schema_name = "public", table_name = "prompt_ferry_config_revision")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i16,
    #[sea_orm(default_expr = "Expr::cust(\"0\")")]
    pub revision: i64,
    #[sea_orm(column_type = "Text")]
    pub last_notify_txid: Option<String>,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `prompt_ferry_config_revision`.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check(Expr::cust("id = 1"));
    statement.check(Expr::cust("revision >= 0"));
    statement
}
