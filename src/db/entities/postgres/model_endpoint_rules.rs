//! Native SeaORM entity for the PostgreSQL `model_endpoint_rules` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `model_endpoint_rules(scope, owner_user_id, enabled, priority)` index.
pub const SCOPE_USER_INDEX: &str = "idx_model_endpoint_rules_scope_user";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "model_endpoint_rules")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"(md5(random()::text || clock_timestamp()::text))::uuid\")"
    )]
    pub rule_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub scope: String,
    #[sea_orm(column_type = "BigInteger")]
    pub owner_user_id: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub model_pattern: String,
    #[sea_orm(column_type = "Uuid")]
    pub endpoint_id: Uuid,
    #[sea_orm(column_type = "Integer", default_value = "100")]
    pub priority: i32,
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
    #[sea_orm(column_type = "Text", default_value = "client_key_rendezvous")]
    pub routing_strategy: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `model_endpoint_rules`.
///
/// The scope and owner checks are table level constraints in the applied
/// schema and are therefore declared natively; the applied foreign keys stay
/// owned by the migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "model_endpoint_rules_scope_check",
        Expr::col(Column::Scope).is_in(["admin", "user"]),
    ));
    statement.check((
        "ck_model_route_owner",
        Expr::col(Column::Scope)
            .eq("admin")
            .and(Expr::col(Column::OwnerUserId).is_null())
            .or(Expr::col(Column::Scope)
                .eq("user")
                .and(Expr::col(Column::OwnerUserId).is_not_null())),
    ));
    statement.check((
        "ck_model_endpoint_rules_routing_strategy",
        Expr::col(Column::RoutingStrategy).eq("responses_session_affinity"),
    ));
    statement
}

/// Native index declarations for `model_endpoint_rules`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(SCOPE_USER_INDEX)
            .table(Entity)
            .col(Column::Scope)
            .col(Column::OwnerUserId)
            .col(Column::Enabled)
            .col(Column::Priority)
            .to_owned(),
    ]
}
