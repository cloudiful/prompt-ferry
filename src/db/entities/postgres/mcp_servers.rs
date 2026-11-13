//! Native SeaORM entity for the PostgreSQL `mcp_servers` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{
    ConditionalStatement, Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement,
};
use sea_orm::{DbBackend, Schema};

/// Unique index name of the applied `mcp_servers.source_endpoint_id` constraint.
pub const SOURCE_ENDPOINT_UNIQUE_INDEX: &str = "idx_mcp_servers_source_endpoint";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "mcp_servers")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"md5(random()::text || clock_timestamp()::text)::uuid\")"
    )]
    pub server_id: Uuid,
    #[sea_orm(unique, column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub transport: String,
    #[sea_orm(column_type = "Text")]
    pub url: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub command: Option<String>,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub args: Json,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'{}'::jsonb\")"
    )]
    pub env_json: Json,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub enabled: bool,
    #[sea_orm(column_type = "Integer", default_expr = "Expr::cust(\"30000\")")]
    pub timeout_ms: i32,
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
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub disabled_tools: Json,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub disabled_resources: Json,
    #[sea_orm(column_type = "Text", default_value = "admin")]
    pub scope: String,
    #[sea_orm(column_type = "BigInteger")]
    pub owner_user_id: Option<i64>,
    #[sea_orm(column_type = "Text", default_value = "blacklist")]
    pub tool_filter_mode: String,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub allowed_tools: Json,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'{}'::jsonb\")"
    )]
    pub http_headers_json: Json,
    #[sea_orm(column_type = "Text", default_value = "passthrough_preferred")]
    pub aggregate_naming_mode: String,
    #[sea_orm(
        column_type = "JsonBinary",
        default_expr = "Expr::cust(\"'[]'::jsonb\")"
    )]
    pub bearer_tokens_json: Json,
    #[sea_orm(column_type = "Text", default_value = "auto")]
    pub lifecycle_policy: String,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_manual_protocol_version: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_mode: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub lifecycle_learned_protocol_version: Option<String>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub lifecycle_learned_for_updated_at: Option<DateTimeUtc>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub lifecycle_learned_at: Option<DateTimeUtc>,
    #[sea_orm(column_type = "Uuid")]
    pub source_endpoint_id: Option<Uuid>,
    #[sea_orm(column_type = "Text", default_value = "none")]
    pub auth_mode: String,
    #[sea_orm(column_type = "Text")]
    pub basic_username: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub basic_password: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub provider_kind: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub proxy_url: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `mcp_servers`.
///
/// The applied table carries named table level checks; they are declared
/// natively because entity attributes cannot express a check constraint. The
/// applied foreign keys to `users` and `provider_endpoints` stay owned by the
/// migration contract, so no `Relation` is declared here.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "ck_mcp_server_scope",
        Expr::col(Column::Scope).is_in(["admin", "user"]),
    ));
    statement.check((
        "ck_mcp_server_owner",
        Expr::col(Column::Scope)
            .eq("admin")
            .and(Expr::col(Column::OwnerUserId).is_null())
            .or(Expr::col(Column::Scope)
                .eq("user")
                .and(Expr::col(Column::OwnerUserId).is_not_null())),
    ));
    statement.check((
        "ck_mcp_server_transport",
        Expr::col(Column::Transport).is_in(["http", "stdio", "builtin_minimax"]),
    ));
    statement.check((
        "ck_mcp_server_tool_filter_mode",
        Expr::col(Column::ToolFilterMode).is_in(["blacklist", "whitelist"]),
    ));
    statement.check((
        "ck_mcp_servers_aggregate_naming_mode",
        Expr::col(Column::AggregateNamingMode).is_in(["qualified_only", "passthrough_preferred"]),
    ));
    statement.check((
        "ck_mcp_server_lifecycle_policy",
        Expr::col(Column::LifecyclePolicy).is_in(["auto", "legacy_initialize"]),
    ));
    statement.check((
        "ck_mcp_server_lifecycle_learned_mode",
        Expr::col(Column::LifecycleLearnedMode)
            .is_null()
            .or(Expr::col(Column::LifecycleLearnedMode)
                .is_in(["modern_discover", "legacy_initialize"])),
    ));
    statement.check((
        "ck_mcp_server_auth_mode",
        Expr::col(Column::AuthMode).is_in(["none", "bearer", "basic", "oauth"]),
    ));
    statement.check((
        "ck_mcp_server_provider_kind",
        Expr::col(Column::ProviderKind).is_in(["generic", "minimax", "context7", "firecrawl"]),
    ));
    statement
}

/// Native index declarations for `mcp_servers`.
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
            .name("idx_mcp_servers_enabled")
            .table(Entity)
            .col(Column::Enabled)
            .col(Column::Name)
            .to_owned(),
        Index::create()
            .name("idx_mcp_servers_scope_user_enabled")
            .table(Entity)
            .col(Column::Scope)
            .col(Column::OwnerUserId)
            .col(Column::Enabled)
            .col(Column::Name)
            .to_owned(),
    ]
}
