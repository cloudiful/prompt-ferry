//! Native SeaORM entity for the PostgreSQL `mcp_credentials` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

/// Unique index name of the applied `mcp_credentials(server_id, position)` constraint.
pub const SERVER_POSITION_UNIQUE_INDEX: &str = "idx_mcp_credentials_server_position";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mcp_credentials")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"md5(random()::text || clock_timestamp()::text)::uuid\")"
    )]
    pub credential_id: Uuid,
    #[sea_orm(column_type = "Uuid")]
    pub server_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub credential_label: String,
    #[sea_orm(column_type = "Text")]
    pub secret: String,
    #[sea_orm(column_type = "Integer")]
    pub position: i32,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub enabled: bool,
    #[sea_orm(column_type = "Text")]
    pub provider_kind: Option<String>,
    #[sea_orm(column_type = "Double", default_expr = "Expr::cust(\"1\")")]
    pub default_cost: f64,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub strict_mode: bool,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub billing_period_start: Option<DateTimeUtc>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub billing_period_end: Option<DateTimeUtc>,
    #[sea_orm(column_type = "Double")]
    pub provider_remaining: Option<f64>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub provider_synced_at: Option<DateTimeUtc>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub provider_reset_at: Option<DateTimeUtc>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub cooldown_until: Option<DateTimeUtc>,
    #[sea_orm(column_type = "Text")]
    pub last_error: Option<String>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub last_error_at: Option<DateTimeUtc>,
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

/// Native schema declaration for `mcp_credentials`.
///
/// The applied checks are named table level constraints and are declared
/// natively; the applied foreign key to `mcp_servers` stays owned by the
/// migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "ck_mcp_credentials_default_cost",
        Expr::col(Column::DefaultCost).gte(0f64),
    ));
    statement.check((
        "ck_mcp_credentials_period",
        Expr::col(Column::BillingPeriodEnd)
            .is_null()
            .or(Expr::col(Column::BillingPeriodStart).is_null())
            .or(Expr::col(Column::BillingPeriodEnd).gt(Expr::col(Column::BillingPeriodStart))),
    ));
    statement
}

/// Native index declarations for `mcp_credentials`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(SERVER_POSITION_UNIQUE_INDEX)
            .table(Entity)
            .col(Column::ServerId)
            .col(Column::Position)
            .unique()
            .to_owned(),
    ]
}
