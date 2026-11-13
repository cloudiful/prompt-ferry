//! Native SeaORM entity for the PostgreSQL `provider_endpoints` table.
//!
//! The declaration mirrors the applied table: the `endpoint_id` UUID default,
//! the `scope`/`owner_user_id`/native API/provider checks and the scope, owner
//! and enabled index are declared here, while the applied foreign key to
//! `users` stays owned by the migration contract and no `Relation` is declared.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "provider_endpoints")]
pub struct Model {
    #[sea_orm(
        primary_key,
        auto_increment = false,
        default_expr = "Expr::cust(\"(md5(random()::text || clock_timestamp()::text)::uuid)\")"
    )]
    pub endpoint_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub scope: String,
    #[sea_orm(column_type = "BigInteger")]
    pub owner_user_id: Option<i64>,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub base_url: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::cust(\"'chat'\")")]
    pub native_api: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::cust(\"'manual'\")")]
    pub native_api_source: String,
    #[sea_orm(column_type = "Text")]
    pub api_key: String,
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
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub key_lb_enabled: bool,
    #[sea_orm(column_type = "Text", default_expr = "Expr::cust(\"'generic'\")")]
    pub provider: String,
    #[sea_orm(column_type = "Text")]
    pub provider_region: Option<String>,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"FALSE\")")]
    pub mcp_enabled: bool,
    #[sea_orm(column_type = "Text")]
    pub service_tier: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub proxy_url: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub active_windows: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub admin_api_key: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `provider_endpoints`.
///
/// The applied table keeps its scope, owner, native API, native API source,
/// provider and provider region checks as named table constraints, so they are
/// declared natively with the applied names and value sets. The applied
/// `service_tier` column is nullable and carries neither a default nor a check.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "provider_endpoints_scope_check",
        Expr::col(Column::Scope).is_in(["admin", "user"]),
    ));
    statement.check((
        "ck_provider_endpoint_owner",
        Expr::col(Column::Scope)
            .eq("admin")
            .and(Expr::col(Column::OwnerUserId).is_null())
            .or(Expr::col(Column::Scope)
                .eq("user")
                .and(Expr::col(Column::OwnerUserId).is_not_null())),
    ));
    statement.check((
        "ck_provider_endpoints_native_api",
        Expr::col(Column::NativeApi).is_in([
            "auto",
            "responses",
            "chat",
            "anthropic_messages",
            "embeddings",
        ]),
    ));
    statement.check((
        "ck_provider_endpoints_native_api_source",
        Expr::col(Column::NativeApiSource).is_in(["auto", "detected", "manual"]),
    ));
    statement.check((
        "ck_provider_endpoints_provider",
        Expr::col(Column::Provider).is_in([
            "generic",
            "minimax",
            "command_code",
            "opencode_go",
            "openrouter",
            "glm",
            "deepseek",
            "openai",
        ]),
    ));
    statement.check((
        "ck_provider_endpoints_provider_region",
        provider_region_without_region()
            .or(minimax_with_region())
            .or(provider_without_region("command_code"))
            .or(provider_without_region("opencode_go"))
            .or(provider_without_region("openrouter"))
            .or(provider_without_region("glm"))
            .or(provider_without_region("deepseek"))
            .or(provider_without_region("openai")),
    ));
    statement
}

fn provider_without_region(provider: &str) -> sea_orm::sea_query::SimpleExpr {
    Expr::col(Column::Provider)
        .eq(provider)
        .and(Expr::col(Column::ProviderRegion).is_null())
}

fn provider_region_without_region() -> sea_orm::sea_query::SimpleExpr {
    provider_without_region("generic")
}

fn minimax_with_region() -> sea_orm::sea_query::SimpleExpr {
    Expr::col(Column::Provider)
        .eq("minimax")
        .and(Expr::col(Column::ProviderRegion).is_not_null())
        .and(Expr::col(Column::ProviderRegion).is_in(["cn", "global"]))
}

/// Native index declarations for `provider_endpoints`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name("idx_provider_endpoints_scope_user")
            .table(Entity)
            .col(Column::Scope)
            .col(Column::OwnerUserId)
            .col(Column::Enabled)
            .to_owned(),
    ]
}
