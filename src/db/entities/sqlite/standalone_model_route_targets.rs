//! Native SeaORM entity for the standalone SQLite
//! `standalone_model_route_targets` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    ConditionalStatement, Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Index name of the applied `standalone_model_route_targets(rule_id, enabled, position)` index.
pub const RULE_INDEX: &str = "idx_standalone_route_targets_rule";
/// Unique index name of the applied per-rule new-session preference.
pub const PREFERRED_NEW_SESSION_UNIQUE_INDEX: &str = "ux_standalone_model_route_targets_preferred";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_model_route_targets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub target_id: String,
    #[sea_orm(column_type = "Text")]
    pub rule_id: String,
    #[sea_orm(column_type = "Text")]
    pub endpoint_id: String,
    #[sea_orm(column_type = "Integer")]
    pub position: i32,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Text")]
    pub upstream_model: Option<String>,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_override_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_override_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub proxy_url_override_key_version: Option<i32>,
    #[sea_orm(column_type = "Text")]
    pub active_windows: Option<String>,
    #[sea_orm(column_type = "Integer", default_value = "0")]
    pub dev_system_normalize: i32,
    #[sea_orm(column_type = "Text", default_value = "auto")]
    pub native_api: String,
    #[sea_orm(column_type = "Text")]
    pub thinking_effort_override: Option<String>,
    #[sea_orm(column_type = "Text", default_value = "passthrough")]
    pub compact_mode: String,
    #[sea_orm(column_type = "Integer", default_value = "0")]
    pub thinking_downgrade_enabled: i32,
    #[sea_orm(column_type = "Text")]
    pub service_tier: Option<String>,
    #[sea_orm(column_type = "Integer")]
    pub embedding_dimensions_override: Option<i32>,
    #[sea_orm(column_type = "Integer", default_value = "0")]
    pub prefer_new_sessions: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_model_route_targets`.
///
/// The enabled and dimension checks are table level constraints in the applied
/// schema and are therefore declared natively; the applied foreign keys stay
/// owned by the standalone migration contract, so no `Relation` is declared
/// here. The applied rule and position uniqueness is an unnamed table
/// constraint and is declared the same way.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_model_route_targets_enabled_check",
        Expr::col(Column::Enabled).is_in([0, 1]),
    ));
    statement.check((
        "standalone_model_route_targets_embedding_dimensions_override_check",
        Expr::col(Column::EmbeddingDimensionsOverride)
            .is_null()
            .or(Expr::cust(
                "typeof(\"standalone_model_route_targets\".\"embedding_dimensions_override\") = 'integer'",
            )
            .and(Expr::col(Column::EmbeddingDimensionsOverride).gt(0))
            .and(Expr::col(Column::EmbeddingDimensionsOverride).lte(2147483647))),
    ));
    let mut unique_rule_position = Index::create()
        .table(Entity)
        .col(Column::RuleId)
        .col(Column::Position)
        .unique()
        .to_owned();
    statement.index(&mut unique_rule_position);
    statement
}

/// Native index declarations for `standalone_model_route_targets`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    let mut preferred = Index::create()
        .name(PREFERRED_NEW_SESSION_UNIQUE_INDEX)
        .table(Entity)
        .col(Column::RuleId)
        .unique()
        .to_owned();
    preferred.and_where(Expr::col(Column::PreferNewSessions).eq(1));

    vec![
        Index::create()
            .name(RULE_INDEX)
            .table(Entity)
            .col(Column::RuleId)
            .col(Column::Enabled)
            .col(Column::Position)
            .to_owned(),
        preferred,
    ]
}
