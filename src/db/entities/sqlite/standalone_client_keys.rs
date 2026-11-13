//! Native SeaORM entity for the standalone SQLite `standalone_client_keys` table.
//!
//! The applied `key_id` is a nullable `TEXT PRIMARY KEY` in an ordinary SQLite
//! rowid table, which the entity derived declaration cannot express because the
//! typed model keeps a nonoptional UUID string. The table is therefore declared
//! manually with the same columns, defaults and check as the applied migration;
//! the declaration is descriptive and the migration stays the only applier.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Table, TableCreateStatement};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_client_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub key_id: String,
    #[sea_orm(column_type = "Integer")]
    pub user_id: i64,
    #[sea_orm(column_type = "Text")]
    pub key_prefix: String,
    #[sea_orm(column_type = "Text")]
    pub label: String,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Blob")]
    pub secret_ciphertext: Vec<u8>,
    #[sea_orm(column_type = "Blob")]
    pub secret_nonce: Vec<u8>,
    #[sea_orm(column_type = "Integer")]
    pub secret_key_version: i32,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_client_keys`.
///
/// `key_id` keeps the applied nullable `TEXT PRIMARY KEY` shape (no `NOT NULL`),
/// while the typed entity remains nonoptional for valid domain keys. The applied
/// foreign key to `standalone_users` stays owned by the standalone migration
/// contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut updated_at = ColumnDef::new(Column::UpdatedAt);
    updated_at
        .text()
        .not_null()
        .default(Expr::current_timestamp());

    Table::create()
        .table(Entity)
        .col(
            ColumnDef::new(Column::KeyId)
                .text()
                .primary_key()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::UserId)
                .integer()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::KeyPrefix)
                .text()
                .not_null()
                .to_owned(),
        )
        .col(ColumnDef::new(Column::Label).text().not_null().to_owned())
        .col(
            ColumnDef::new(Column::Enabled)
                .integer()
                .not_null()
                .check(Expr::col(Column::Enabled).is_in([0, 1]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::SecretCiphertext)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::SecretNonce)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::SecretKeyVersion)
                .integer()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::CreatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp())
                .to_owned(),
        )
        .col(updated_at.to_owned())
        .to_owned()
}
