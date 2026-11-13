use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "conversation_endpoint_overrides")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub conversation_id: Uuid,
    pub endpoint_id: Uuid,
    pub created_by_user_id: Option<i64>,
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
    pub endpoint_key_id: Option<Uuid>,
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
            .name("idx_conversation_endpoint_overrides_endpoint_id")
            .table(Entity)
            .col(Column::EndpointId)
            .to_owned(),
        Index::create()
            .name("idx_conversation_endpoint_overrides_endpoint_key_id")
            .table(Entity)
            .col(Column::EndpointKeyId)
            .to_owned(),
    ]
}
