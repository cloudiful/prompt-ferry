use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(schema_name = "public", table_name = "prompt_ferry_node_gate")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    #[sea_orm(column_type = "Text")]
    pub state: String,
    #[sea_orm(column_type = "Text")]
    pub runtime_version: String,
    pub protocol_version: i32,
    pub generation: i64,
    #[sea_orm(column_type = "Text")]
    pub target_runtime_version: Option<String>,
    pub target_protocol_version: Option<i32>,
    pub owner_id: Option<Uuid>,
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
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check(Expr::cust("id = 1"));
    statement.check(Expr::cust(
        "state IN ('ready', 'draining', 'migrating', 'failed')",
    ));
    statement.check(Expr::cust("protocol_version >= 0 AND generation >= 0"));
    statement
}
