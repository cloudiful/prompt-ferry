//! Native SeaORM entity for the PostgreSQL `model_route_targets` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    ConditionalStatement, Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Unique index name of the applied `model_route_targets(rule_id, position)` constraint.
pub const RULE_POSITION_UNIQUE_INDEX: &str = "idx_model_route_targets_rule_position";
/// Index name of the applied `model_route_targets(endpoint_id, enabled)` index.
pub const ENDPOINT_ENABLED_INDEX: &str = "idx_model_route_targets_endpoint_enabled";
/// Unique index name of the applied per-rule new-session preference.
pub const PREFERRED_NEW_SESSION_UNIQUE_INDEX: &str = "ux_model_route_targets_preferred_new_session";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "model_route_targets")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"(md5(random()::text || clock_timestamp()::text))::uuid\")"
    )]
    pub target_id: Uuid,
    #[sea_orm(column_type = "Uuid")]
    pub rule_id: Uuid,
    #[sea_orm(column_type = "Uuid")]
    pub endpoint_id: Uuid,
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
    #[sea_orm(column_type = "Text")]
    pub upstream_model: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub proxy_url_override: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub active_windows: Option<String>,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub dev_system_normalize: bool,
    #[sea_orm(column_type = "Text", default_value = "auto")]
    pub native_api: String,
    #[sea_orm(column_type = "Text")]
    pub thinking_effort_override: Option<String>,
    #[sea_orm(column_type = "Text", default_value = "passthrough")]
    pub compact_mode: String,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub thinking_downgrade_enabled: bool,
    #[sea_orm(column_type = "Text")]
    pub service_tier: Option<String>,
    #[sea_orm(column_type = "Integer")]
    pub embedding_dimensions_override: Option<i32>,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub prefer_new_sessions: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `model_route_targets`.
///
/// The protocol, compact mode, thinking effort and dimension checks are table
/// level constraints in the applied schema and are therefore declared
/// natively; the applied foreign keys stay owned by the migration contract, so
/// no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "ck_model_route_targets_native_api",
        Expr::col(Column::NativeApi).is_in([
            "auto",
            "responses",
            "chat",
            "anthropic_messages",
            "realtime",
            "embeddings",
        ]),
    ));
    statement.check((
        "ck_model_route_targets_compact_mode",
        Expr::col(Column::CompactMode).is_in(["passthrough", "self_summarize", "off"]),
    ));
    statement.check((
        "ck_model_route_targets_thinking_effort",
        Expr::col(Column::ThinkingEffortOverride)
            .is_null()
            .or(Expr::col(Column::ThinkingEffortOverride)
                .is_in(["none", "minimal", "low", "medium", "high", "xhigh", "max"])),
    ));
    statement.check((
        "ck_model_route_targets_embedding_dimensions_override",
        Expr::col(Column::EmbeddingDimensionsOverride)
            .is_null()
            .or(Expr::col(Column::EmbeddingDimensionsOverride).gt(0)),
    ));
    statement
}

/// Native index declarations for `model_route_targets`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    let mut preferred = Index::create()
        .name(PREFERRED_NEW_SESSION_UNIQUE_INDEX)
        .table(Entity)
        .col(Column::RuleId)
        .unique()
        .to_owned();
    preferred.and_where(Expr::col(Column::PreferNewSessions));

    vec![
        Index::create()
            .name(RULE_POSITION_UNIQUE_INDEX)
            .table(Entity)
            .col(Column::RuleId)
            .col(Column::Position)
            .unique()
            .to_owned(),
        Index::create()
            .name(ENDPOINT_ENABLED_INDEX)
            .table(Entity)
            .col(Column::EndpointId)
            .col(Column::Enabled)
            .to_owned(),
        preferred,
    ]
}
