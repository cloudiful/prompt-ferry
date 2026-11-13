//! Native SeaORM entity for the standalone SQLite `standalone_model_routes` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `standalone_model_routes(scope, owner_user_id, enabled)` index.
pub const SCOPE_OWNER_ENABLED_INDEX: &str = "idx_standalone_routes_enabled";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_model_routes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub rule_id: String,
    #[sea_orm(column_type = "Text")]
    pub scope: String,
    #[sea_orm(column_type = "BigInteger")]
    pub owner_user_id: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub model_pattern: String,
    #[sea_orm(column_type = "Text")]
    pub routing_strategy: String,
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

/// Native schema declaration for `standalone_model_routes`.
///
/// The scope, routing strategy, enabled and owner checks are table level
/// constraints in the applied schema and are therefore declared natively; the
/// applied foreign keys stay owned by the standalone migration contract, so no
/// `Relation` is declared here. The applied scope, owner and pattern
/// uniqueness is an unnamed table constraint and is declared the same way.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_model_routes_scope_check",
        Expr::col(Column::Scope).is_in(["admin", "user"]),
    ));
    statement.check((
        "standalone_model_routes_routing_strategy_check",
        Expr::col(Column::RoutingStrategy).is_in(["responses_session_affinity"]),
    ));
    statement.check((
        "standalone_model_routes_enabled_check",
        Expr::col(Column::Enabled).is_in([0, 1]),
    ));
    statement.check((
        "standalone_model_routes_owner_check",
        Expr::col(Column::Scope)
            .eq("admin")
            .and(Expr::col(Column::OwnerUserId).is_null())
            .or(Expr::col(Column::Scope)
                .eq("user")
                .and(Expr::col(Column::OwnerUserId).is_not_null())),
    ));
    let mut unique_scope_owner_pattern = Index::create()
        .table(Entity)
        .col(Column::Scope)
        .col(Column::OwnerUserId)
        .col(Column::ModelPattern)
        .unique()
        .to_owned();
    statement.index(&mut unique_scope_owner_pattern);
    statement
}

/// Native index declarations for `standalone_model_routes`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(SCOPE_OWNER_ENABLED_INDEX)
            .table(Entity)
            .col(Column::Scope)
            .col(Column::OwnerUserId)
            .col(Column::Enabled)
            .to_owned(),
    ]
}
