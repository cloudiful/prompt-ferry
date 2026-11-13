//! Native SeaORM entity for the standalone SQLite `standalone_relays` table.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, TableCreateStatement};
use sea_orm::{DbBackend, Schema};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "standalone_relays")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub relay_id: String,
    #[sea_orm(unique, column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub relay_url: String,
    #[sea_orm(column_type = "Integer")]
    pub enabled: i32,
    #[sea_orm(column_type = "Text")]
    pub tls_mode: String,
    #[sea_orm(column_type = "Text")]
    pub bridge_encryption_mode: String,
    #[sea_orm(column_type = "Blob")]
    pub relay_ca_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub relay_ca_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub relay_ca_key_version: Option<i32>,
    #[sea_orm(column_type = "Blob")]
    pub client_cert_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub client_cert_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub client_cert_key_version: Option<i32>,
    #[sea_orm(column_type = "Blob")]
    pub client_key_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub client_key_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub client_key_key_version: Option<i32>,
    #[sea_orm(column_type = "Blob")]
    pub bridge_encryption_key_ciphertext: Option<Vec<u8>>,
    #[sea_orm(column_type = "Blob")]
    pub bridge_encryption_key_nonce: Option<Vec<u8>>,
    #[sea_orm(column_type = "Integer")]
    pub bridge_encryption_key_key_version: Option<i32>,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub created_at: String,
    #[sea_orm(column_type = "Text", default_expr = "Expr::current_timestamp()")]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Native schema declaration for `standalone_relays`.
///
/// The applied table carries the mode checks and the four envelope consistency
/// checks as table level constraints; they are declared natively because
/// entity attributes cannot express a check constraint.
pub fn create_table_statement() -> TableCreateStatement {
    let mut statement = Schema::new(DbBackend::Sqlite).create_table_from_entity(Entity);
    statement.check((
        "standalone_relays_enabled_check",
        Expr::col(Column::Enabled).is_in([0, 1]),
    ));
    statement.check((
        "standalone_relays_tls_mode_check",
        Expr::col(Column::TlsMode).is_in(["off", "server", "mtls"]),
    ));
    statement.check((
        "standalone_relays_bridge_encryption_mode_check",
        Expr::col(Column::BridgeEncryptionMode).is_in(["off", "required"]),
    ));
    for (label, ciphertext, nonce, key_version) in [
        (
            "relay_ca",
            Column::RelayCaCiphertext,
            Column::RelayCaNonce,
            Column::RelayCaKeyVersion,
        ),
        (
            "client_cert",
            Column::ClientCertCiphertext,
            Column::ClientCertNonce,
            Column::ClientCertKeyVersion,
        ),
        (
            "client_key",
            Column::ClientKeyCiphertext,
            Column::ClientKeyNonce,
            Column::ClientKeyKeyVersion,
        ),
        (
            "bridge_encryption_key",
            Column::BridgeEncryptionKeyCiphertext,
            Column::BridgeEncryptionKeyNonce,
            Column::BridgeEncryptionKeyKeyVersion,
        ),
    ] {
        statement.check((
            format!("standalone_relays_{label}_envelope_check"),
            envelope_condition(ciphertext, nonce, key_version),
        ));
    }
    statement
}

/// Native envelope condition of one secret triplet.
///
/// The applied standalone schema keeps a triplet either completely absent or
/// completely present: ciphertext, nonce and key version agree on nullness.
/// The condition is built from that rule instead of from a nullness mismatch,
/// because a mismatch would accept exactly the incomplete triplets the
/// applied schema rejects.
fn envelope_condition(ciphertext: Column, nonce: Column, key_version: Column) -> Expr {
    let absent = Expr::col(ciphertext)
        .is_null()
        .and(Expr::col(nonce).is_null())
        .and(Expr::col(key_version).is_null());
    let present = Expr::col(ciphertext)
        .is_not_null()
        .and(Expr::col(nonce).is_not_null())
        .and(Expr::col(key_version).is_not_null());
    absent.or(present)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use sea_orm::sea_query::{BinOper, ColumnRef, Expr, Keyword};

    use super::{Column, envelope_condition};

    fn column_name(column: &ColumnRef) -> String {
        column
            .column()
            .map(|name| name.to_string())
            .expect("column reference without a name")
    }

    fn is_null(expression: &Expr, values: &HashMap<String, bool>) -> bool {
        match expression {
            Expr::Column(column) => values[&column_name(column)],
            other => panic!("unsupported nullness operand: {other:?}"),
        }
    }

    fn evaluate(expression: &Expr, values: &HashMap<String, bool>) -> bool {
        match expression {
            Expr::Binary(left, BinOper::And, right) => {
                evaluate(left, values) && evaluate(right, values)
            }
            Expr::Binary(left, BinOper::Or, right) => {
                evaluate(left, values) || evaluate(right, values)
            }
            Expr::Binary(left, BinOper::Is, right) => {
                assert!(matches!(**right, Expr::Keyword(Keyword::Null)));
                is_null(left, values)
            }
            Expr::Binary(left, BinOper::IsNot, right) => {
                assert!(matches!(**right, Expr::Keyword(Keyword::Null)));
                !is_null(left, values)
            }
            other => panic!("unsupported condition: {other:?}"),
        }
    }

    fn triplet_accepts(ciphertext_null: bool, nonce_null: bool, key_version_null: bool) -> bool {
        let condition = envelope_condition(
            Column::RelayCaCiphertext,
            Column::RelayCaNonce,
            Column::RelayCaKeyVersion,
        );
        let values = HashMap::from([
            ("relay_ca_ciphertext".to_string(), ciphertext_null),
            ("relay_ca_nonce".to_string(), nonce_null),
            ("relay_ca_key_version".to_string(), key_version_null),
        ]);
        evaluate(&condition, &values)
    }

    #[test]
    fn envelope_condition_accepts_only_complete_triplets() {
        assert!(triplet_accepts(true, true, true));
        assert!(triplet_accepts(false, false, false));

        assert!(!triplet_accepts(true, false, false));
        assert!(!triplet_accepts(false, true, false));
        assert!(!triplet_accepts(false, false, true));
        assert!(!triplet_accepts(true, false, true));
        assert!(!triplet_accepts(false, true, true));
        assert!(!triplet_accepts(true, true, false));
    }
}
