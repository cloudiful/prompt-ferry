//! Native SeaORM entity for the standalone SQLite `standalone_mcp_servers` table.
//!
//! The standalone store keeps no credential table: the bearer token envelope
//! lives on the server row, so there is no SQLite credential entity.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    ConditionalStatement, Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Unique index name of the applied `standalone_mcp_servers.source_endpoint_id` constraint.
pub const SOURCE_ENDPOINT_UNIQUE_INDEX: &str = "idx_standalone_mcp_servers_source_endpoint";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_mcp_servers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub server_id: String,
    #[sea_orm(column_type = "Text")]
    pub source_endpoint_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub scope: String,
    #[sea_orm(column_type = "BigInteger")]
    pub owner_user_id: Option<i64>,
    #[sea_orm(unique, column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub aggregate_naming_mode: String,
    #[sea_orm(column_type = "Text")]
    pub transport: String,
    #[sea_orm(column_type = "Text")]
    pub provider_kind: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub url: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub command: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub args_json: String,
    #[sea_orm(column_type = "Text")]
    pub http_headers_json: String,
    #[sea_orm(column_type = "Text", default_value = "none")]
    pub auth_mode: String,
    #[sea_orm(column_type = "Text")]
    pub basic_username: Option<String>,
    #[sea_orm(column_type = "Blob")]
    pub basic_password_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub basic_password_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub basic_password_key_version: Option<i32>,
    #[sea_orm(column_type = "Text")]
    pub tool_filter_mode: String,
    #[sea_orm(column_type = "Text")]
    pub allowed_tools_json: String,
    #[sea_orm(column_type = "Text")]
    pub disabled_tools_json: String,
    #[sea_orm(column_type = "Text")]
    pub disabled_resources_json: String,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Integer")]
    pub timeout_ms: i32,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_policy: String,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_manual_protocol_version: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_mode: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_protocol_version: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_for_updated_at: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_at: Option<String>,
    #[sea_orm(column_type = "Blob")]
    pub env_ciphertext: Vec<u8>,
    #[sea_orm(column_type = "Blob")]
    pub env_nonce: Vec<u8>,
    #[sea_orm(column_type = "Integer")]
    pub env_key_version: i32,
    #[sea_orm(column_type = "Blob")]
    pub bearer_tokens_ciphertext: Vec<u8>,
    #[sea_orm(column_type = "Blob")]
    pub bearer_tokens_nonce: Vec<u8>,
    #[sea_orm(column_type = "Integer")]
    pub bearer_tokens_key_version: i32,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub proxy_url_key_version: Option<i32>,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_mcp_servers`.
///
/// The applied table carries the scope, transport, filter mode, naming mode,
/// lifecycle, timeout and source endpoint checks as table level constraints;
/// they are declared natively because entity attributes cannot express a
/// check constraint. The applied foreign keys stay owned by the standalone
/// migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_mcp_servers_scope_check",
        Expr::col(Column::Scope).is_in(["admin", "user"]),
    ));
    statement.check((
        "standalone_mcp_servers_owner_check",
        Expr::col(Column::Scope)
            .eq("admin")
            .and(Expr::col(Column::OwnerUserId).is_null())
            .or(Expr::col(Column::Scope)
                .eq("user")
                .and(Expr::col(Column::OwnerUserId).is_not_null())),
    ));
    statement.check((
        "standalone_mcp_servers_aggregate_naming_mode_check",
        Expr::col(Column::AggregateNamingMode).is_in(["qualified_only", "passthrough_preferred"]),
    ));
    statement.check((
        "standalone_mcp_servers_transport_check",
        Expr::col(Column::Transport).is_in(["http", "stdio", "builtin_minimax"]),
    ));
    statement.check((
        "standalone_mcp_servers_tool_filter_mode_check",
        Expr::col(Column::ToolFilterMode).is_in(["blacklist", "whitelist"]),
    ));
    statement.check((
        "standalone_mcp_servers_lifecycle_policy_check",
        Expr::col(Column::LifecyclePolicy).is_in(["auto", "legacy_initialize"]),
    ));
    statement.check((
        "standalone_mcp_servers_enabled_check",
        Expr::col(Column::Enabled).is_in([0, 1]),
    ));
    statement.check((
        "standalone_mcp_servers_timeout_ms_check",
        Expr::col(Column::TimeoutMs).between(100, 300000),
    ));
    statement.check((
        "standalone_mcp_servers_source_endpoint_check",
        Expr::col(Column::SourceEndpointId)
            .is_null()
            .or(Expr::col(Column::Transport).eq("builtin_minimax")),
    ));
    statement
}

/// Native index declarations for `standalone_mcp_servers`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    let mut source_endpoint = Index::create()
        .name(SOURCE_ENDPOINT_UNIQUE_INDEX)
        .table(Entity)
        .col(Column::SourceEndpointId)
        .unique()
        .to_owned();
    source_endpoint.and_where(Expr::col(Column::SourceEndpointId).is_not_null());

    vec![
        source_endpoint,
        Index::create()
            .name("idx_standalone_mcp_servers_visible")
            .table(Entity)
            .col(Column::Enabled)
            .col(Column::Scope)
            .col(Column::OwnerUserId)
            .col(Column::Name)
            .to_owned(),
    ]
}
