//! Native SeaORM entity for the standalone SQLite `standalone_users` table.
//!
//! The applied `login_name` collation is `COLLATE NOCASE`, which the entity
//! derived declaration cannot express on sea-query 1.0.2. The table is
//! therefore declared manually with the same columns, checks, defaults and
//! index as the applied migration; the declaration is descriptive and the
//! migration stays the only applier.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    ColumnDef, Expr, ExprTrait, Index, IndexCreateStatement, Table, TableCreateStatement,
};

/// Index name of the applied `standalone_users(enabled, is_admin, login_name)` index.
pub const ENABLED_ADMIN_INDEX: &str = "idx_standalone_users_enabled_admin";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Integer")]
    pub user_id: i64,
    #[sea_orm(column_type = "Text", unique)]
    pub login_name: String,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Text")]
    pub password_hash: String,
    #[sea_orm(column_type = "Integer")]
    pub is_admin: i32,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_users`.
///
/// `login_name` is declared manually so that the applied `COLLATE NOCASE`
/// survives in the rendered declaration; the boolean checks are table level
/// constraints in the applied schema. Foreign keys to this table stay owned by
/// the standalone migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut login_name = ColumnDef::new(Column::LoginName);
    login_name
        .text()
        .not_null()
        .unique_key()
        .extra("COLLATE NOCASE");

    let mut is_admin = ColumnDef::new(Column::IsAdmin);
    is_admin
        .integer()
        .not_null()
        .check(Expr::col(Column::IsAdmin).is_in([0, 1]));

    let mut enabled = ColumnDef::new(Column::Enabled);
    enabled
        .integer()
        .not_null()
        .check(Expr::col(Column::Enabled).is_in([0, 1]));

    let mut created_at = ColumnDef::new(Column::CreatedAt);
    created_at
        .text()
        .not_null()
        .default(Expr::current_timestamp());

    let mut updated_at = ColumnDef::new(Column::UpdatedAt);
    updated_at
        .text()
        .not_null()
        .default(Expr::current_timestamp());

    Table::create()
        .table(Entity)
        .col(
            ColumnDef::new(Column::UserId)
                .integer()
                .primary_key()
                .to_owned(),
        )
        .col(login_name.to_owned())
        .col(
            ColumnDef::new(Column::DisplayName)
                .text()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::PasswordHash)
                .text()
                .not_null()
                .to_owned(),
        )
        .col(is_admin.to_owned())
        .col(enabled.to_owned())
        .col(created_at.to_owned())
        .col(updated_at.to_owned())
        .to_owned()
}

/// Native index declarations for `standalone_users`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(ENABLED_ADMIN_INDEX)
            .table(Entity)
            .col(Column::Enabled)
            .col(Column::IsAdmin)
            .col(Column::LoginName)
            .to_owned(),
    ]
}
