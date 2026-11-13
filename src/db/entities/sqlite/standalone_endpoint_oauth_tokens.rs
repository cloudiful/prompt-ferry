use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{ColumnDef, Expr, Table, TableCreateStatement};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_endpoint_oauth_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub endpoint_id: String,
    #[sea_orm(column_type = "Blob")]
    pub access_token_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub access_token_nonce: Option<Vec<u8>>,
    pub access_token_key_version: Option<i64>,
    #[sea_orm(column_type = "Blob")]
    pub refresh_token_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub refresh_token_nonce: Option<Vec<u8>>,
    pub refresh_token_key_version: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub expires_at: Option<String>,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

pub fn create_table_statement() -> TableCreateStatement {
    Table::create()
        .table(Entity)
        .col(ColumnDef::new(Column::EndpointId).text().primary_key())
        .col(ColumnDef::new(Column::AccessTokenCiphertext).blob())
        .col(ColumnDef::new(Column::AccessTokenNonce).blob())
        .col(ColumnDef::new(Column::AccessTokenKeyVersion).integer())
        .col(ColumnDef::new(Column::RefreshTokenCiphertext).blob())
        .col(ColumnDef::new(Column::RefreshTokenNonce).blob())
        .col(ColumnDef::new(Column::RefreshTokenKeyVersion).integer())
        .col(ColumnDef::new(Column::ExpiresAt).text())
        .col(
            ColumnDef::new(Column::CreatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp()),
        )
        .col(
            ColumnDef::new(Column::UpdatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp()),
        )
        .to_owned()
}
