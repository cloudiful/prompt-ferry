//! Native SeaORM entity for the standalone SQLite `standalone_endpoint_keys`
//! table.
//!
//! The applied `key_id` is a nullable `TEXT PRIMARY KEY` in an ordinary SQLite
//! rowid table, which the entity derived declaration cannot express because the
//! typed model keeps a nonoptional UUID string. The table is therefore declared
//! manually with the same columns, defaults, uniqueness and checks as the
//! applied migration; the declaration is descriptive and the migration stays
//! the only applier.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Index, Table, TableCreateStatement};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_endpoint_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub key_id: String,
    #[sea_orm(column_type = "Text")]
    pub endpoint_id: String,
    #[sea_orm(column_type = "Text")]
    pub key_label: String,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Integer")]
    pub position: i32,
    #[sea_orm(column_type = "Blob")]
    pub api_key_ciphertext: Vec<u8>,
    #[sea_orm(column_type = "Blob")]
    pub api_key_nonce: Vec<u8>,
    #[sea_orm(column_type = "Integer")]
    pub api_key_key_version: i32,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_endpoint_keys`.
///
/// `key_id` keeps the applied nullable `TEXT PRIMARY KEY` shape (no `NOT NULL`),
/// while the typed entity remains nonoptional for valid domain keys. The
/// applied key label and position uniqueness, the enabled check and the
/// endpoint, enabled and position index are declared here. The applied foreign
/// key to `standalone_provider_endpoints` stays owned by the standalone
/// migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    Table::create()
        .table(Entity)
        .col(
            ColumnDef::new(Column::KeyId)
                .text()
                .primary_key()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::EndpointId)
                .text()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::KeyLabel)
                .text()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::Enabled)
                .integer()
                .not_null()
                .check(Expr::col(Column::Enabled).is_in([0, 1]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::Position)
                .integer()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyCiphertext)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyNonce)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyKeyVersion)
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
        .col(
            ColumnDef::new(Column::UpdatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp())
                .to_owned(),
        )
        .index(
            Index::create()
                .col(Column::EndpointId)
                .col(Column::KeyLabel)
                .unique(),
        )
        .index(
            Index::create()
                .col(Column::EndpointId)
                .col(Column::Position)
                .unique(),
        )
        .to_owned()
}

/// Native index declarations for `standalone_endpoint_keys`.
pub fn create_index_statements() -> Vec<sea_orm::sea_query::IndexCreateStatement> {
    vec![
        Index::create()
            .name("idx_standalone_endpoint_keys_endpoint")
            .table(Entity)
            .col(Column::EndpointId)
            .col(Column::Enabled)
            .col(Column::Position)
            .to_owned(),
    ]
}
