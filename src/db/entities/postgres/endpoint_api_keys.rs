use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "endpoint_api_keys")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"(md5(random()::text || clock_timestamp()::text))::uuid\")"
    )]
    pub key_id: Uuid,
    #[sea_orm(column_type = "Uuid")]
    pub endpoint_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub key_label: String,
    #[sea_orm(column_type = "Text")]
    pub api_key: String,
    #[sea_orm(column_type = "Integer")]
    pub position: i32,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub enabled: bool,
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

pub fn create_table_statement() -> TableCreateStatement {
    Schema::new(DbBackend::Postgres).create_table_from_entity(Entity)
}

pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name("idx_endpoint_api_keys_endpoint_position")
            .table(Entity)
            .col(Column::EndpointId)
            .col(Column::Position)
            .unique()
            .to_owned(),
    ]
}
