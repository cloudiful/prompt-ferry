use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "endpoint_oauth_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub endpoint_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub access_token: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTimeUtc>,
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

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("endpoint_id", &self.endpoint_id)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

pub fn create_table_statement() -> TableCreateStatement {
    Schema::new(DbBackend::Postgres).create_table_from_entity(Entity)
}
