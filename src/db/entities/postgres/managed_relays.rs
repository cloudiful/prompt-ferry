//! Native SeaORM entity for the PostgreSQL `managed_relays` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

/// Unique index name of the applied `managed_relays.relay_url` constraint.
pub const RELAY_URL_UNIQUE_INDEX: &str = "managed_relays_relay_url_unique";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "managed_relays")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub relay_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub relay_url: String,
    #[sea_orm(column_type = "Boolean", default_expr = "Expr::cust(\"TRUE\")")]
    pub enabled: bool,
    #[sea_orm(column_type = "Text")]
    pub tls_mode: String,
    #[sea_orm(column_type = "Text")]
    pub bridge_encryption_mode: String,
    #[sea_orm(column_type = "Binary(1)")]
    pub relay_ca_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(1)")]
    pub relay_ca_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "SmallInteger")]
    pub relay_ca_key_version: Option<i16>,
    #[sea_orm(column_type = "Binary(1)")]
    pub client_cert_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(1)")]
    pub client_cert_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "SmallInteger")]
    pub client_cert_key_version: Option<i16>,
    #[sea_orm(column_type = "Binary(1)")]
    pub client_key_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(1)")]
    pub client_key_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "SmallInteger")]
    pub client_key_key_version: Option<i16>,
    #[sea_orm(column_type = "Binary(1)")]
    pub bridge_encryption_key_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(1)")]
    pub bridge_encryption_key_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "SmallInteger")]
    pub bridge_encryption_key_key_version: Option<i16>,
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

/// Native schema declaration for `managed_relays`.
///
/// The relay mode checks are table level constraints in the applied schema and
/// are therefore declared natively; the relay url uniqueness is an applied
/// unique index and is declared as an index statement.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Postgres).create_table_from_entity(Entity);
    statement.check((
        "managed_relays_tls_mode_check",
        Expr::col(Column::TlsMode).is_in(["off", "server", "mtls"]),
    ));
    statement.check((
        "managed_relays_bridge_encryption_mode_check",
        Expr::col(Column::BridgeEncryptionMode).is_in(["off", "required"]),
    ));
    statement
}

/// Native index declarations for `managed_relays`.
pub fn create_index_statements() -> Vec<IndexCreateStatement> {
    vec![
        Index::create()
            .name(RELAY_URL_UNIQUE_INDEX)
            .table(Entity)
            .col(Column::RelayUrl)
            .unique()
            .to_owned(),
    ]
}
