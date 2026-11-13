//! Native SeaORM entity for the standalone SQLite `standalone_settings` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub setting_key: String,
    #[sea_orm(column_type = "Integer")]
    pub value_version: i64,
    #[sea_orm(column_type = "Text")]
    pub value_json: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_settings`.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_settings_value_version_check",
        Expr::col(Column::ValueVersion).gt(0),
    ));
    statement
}
