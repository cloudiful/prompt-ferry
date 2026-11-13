//! Native SeaORM entity for the standalone SQLite `standalone_provider_endpoints`
//! table.
//!
//! The applied `endpoint_id` is a nullable `TEXT PRIMARY KEY` in an ordinary
//! SQLite rowid table, which the entity derived declaration cannot express
//! because the typed model keeps a nonoptional UUID string. The table is
//! therefore declared manually with the same columns, defaults, uniqueness and
//! checks as the applied migration; the declaration is descriptive and the
//! migration stays the only applier.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{ColumnDef, Expr, ExprTrait, Table, TableCreateStatement};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_provider_endpoints")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub endpoint_id: String,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub provider: String,
    #[sea_orm(column_type = "Text")]
    pub provider_region: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub base_url: String,
    #[sea_orm(column_type = "Text")]
    pub native_api: String,
    #[sea_orm(column_type = "Text")]
    pub native_api_source: String,
    #[sea_orm(column_type = "Integer")]
    pub key_lb_enabled: i32,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Integer")]
    pub mcp_enabled: i32,
    #[sea_orm(column_type = "Blob")]
    pub api_key_ciphertext: Vec<u8>,
    #[sea_orm(column_type = "Blob")]
    pub api_key_nonce: Vec<u8>,
    #[sea_orm(column_type = "Integer")]
    pub api_key_key_version: i32,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub proxy_url_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub proxy_url_key_version: Option<i32>,
    #[sea_orm(column_type = "Text")]
    pub active_windows: Option<String>,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
    #[sea_orm(column_type = "Text")]
    pub service_tier: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_provider_endpoints`.
///
/// `endpoint_id` keeps the applied nullable `TEXT PRIMARY KEY` shape (no
/// `NOT NULL`), while the typed entity remains nonoptional for valid domain
/// keys. The applied name uniqueness, provider, provider region, native API and
/// native API source checks and the boolean checks are declared with their
/// applied value sets. The applied table carries no foreign key.
pub fn create_table_statement() -> TableCreateStatement {
    Table::create()
        .table(Entity)
        .col(
            ColumnDef::new(Column::EndpointId)
                .text()
                .primary_key()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::Name)
                .text()
                .not_null()
                .unique_key()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::Provider)
                .text()
                .not_null()
                .check(Expr::col(Column::Provider).is_in([
                    "generic",
                    "minimax",
                    "command_code",
                    "opencode_go",
                    "openrouter",
                    "glm",
                    "deepseek",
                    "openai",
                ]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ProviderRegion)
                .text()
                .check(
                    Expr::col(Column::ProviderRegion)
                        .is_null()
                        .or(Expr::col(Column::ProviderRegion).is_in(["cn", "global"])),
                )
                .to_owned(),
        )
        .col(ColumnDef::new(Column::BaseUrl).text().not_null().to_owned())
        .col(
            ColumnDef::new(Column::NativeApi)
                .text()
                .not_null()
                .check(Expr::col(Column::NativeApi).is_in([
                    "auto",
                    "anthropic_messages",
                    "chat",
                    "responses",
                    "realtime",
                    "embeddings",
                ]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::NativeApiSource)
                .text()
                .not_null()
                .check(Expr::col(Column::NativeApiSource).is_in(["auto", "detected", "manual"]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::KeyLbEnabled)
                .integer()
                .not_null()
                .check(Expr::col(Column::KeyLbEnabled).is_in([0, 1]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::Enabled)
                .integer()
                .not_null()
                .check(Expr::col(Column::Enabled).is_in([0, 1]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::McpEnabled)
                .integer()
                .not_null()
                .check(Expr::col(Column::McpEnabled).is_in([0, 1]))
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyCiphertext)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyNonce)
                .blob()
                .not_null()
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::ApiKeyKeyVersion)
                .integer()
                .not_null()
                .to_owned(),
        )
        .col(ColumnDef::new(Column::ProxyUrlCiphertext).blob().to_owned())
        .col(ColumnDef::new(Column::ProxyUrlNonce).blob().to_owned())
        .col(
            ColumnDef::new(Column::ProxyUrlKeyVersion)
                .integer()
                .to_owned(),
        )
        .col(ColumnDef::new(Column::ActiveWindows).text().to_owned())
        .col(
            ColumnDef::new(Column::CreatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp())
                .to_owned(),
        )
        .col(
            ColumnDef::new(Column::UpdatedAt)
                .text()
                .not_null()
                .default(Expr::current_timestamp())
                .to_owned(),
        )
        .col(ColumnDef::new(Column::ServiceTier).text().to_owned())
        .to_owned()
}
