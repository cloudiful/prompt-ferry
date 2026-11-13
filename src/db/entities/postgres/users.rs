//! Native SeaORM entity for the PostgreSQL `users` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub user_id: i64,
    #[sea_orm(column_type = "Text", unique)]
    pub login_name: String,
    #[sea_orm(column_type = "Text")]
    pub password_hash: String,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub is_admin: bool,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub is_active: bool,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub created_at: DateTimeUtc,
    #[sea_orm(
        column_type = "TimestampWithTimeZone",
        default_expr = "Expr::current_timestamp()"
    )]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `users`.
///
/// The applied `user_id` is a `BIGSERIAL` and the applied `login_name`
/// uniqueness is a column constraint; both are declared natively. The applied
/// foreign keys that reference `users` stay owned by the migration contract, so
/// no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    Schema::new(DbBackend::Postgres).create_table_from_entity(Entity)
}
