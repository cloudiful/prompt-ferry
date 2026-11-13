use anyhow::Result;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Order};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::entities::postgres::managed_relays;
use crate::db::seaorm::{native_error, postgres_connection};
use crate::db::types::{ManagedRelayInput, ManagedRelayRow};

fn row_from_model(model: managed_relays::Model) -> ManagedRelayRow {
    ManagedRelayRow {
        relay_id: model.relay_id,
        name: model.name,
        relay_url: model.relay_url,
        enabled: model.enabled,
        tls_mode: model.tls_mode,
        bridge_encryption_mode: model.bridge_encryption_mode,
        relay_ca_ciphertext: model.relay_ca_ciphertext,
        relay_ca_nonce: model.relay_ca_nonce,
        relay_ca_key_version: model.relay_ca_key_version,
        client_cert_ciphertext: model.client_cert_ciphertext,
        client_cert_nonce: model.client_cert_nonce,
        client_cert_key_version: model.client_cert_key_version,
        client_key_ciphertext: model.client_key_ciphertext,
        client_key_nonce: model.client_key_nonce,
        client_key_key_version: model.client_key_key_version,
        bridge_encryption_key_ciphertext: model.bridge_encryption_key_ciphertext,
        bridge_encryption_key_nonce: model.bridge_encryption_key_nonce,
        bridge_encryption_key_key_version: model.bridge_encryption_key_key_version,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn active_model(input: ManagedRelayInput) -> managed_relays::ActiveModel {
    managed_relays::ActiveModel {
        name: Set(input.name),
        relay_url: Set(input.relay_url),
        enabled: Set(input.enabled),
        tls_mode: Set(input.tls_mode.as_str().to_string()),
        bridge_encryption_mode: Set(input.bridge_encryption_mode.as_str().to_string()),
        relay_ca_ciphertext: Set(input
            .relay_ca
            .as_ref()
            .map(|value| value.ciphertext.clone())),
        relay_ca_nonce: Set(input.relay_ca.as_ref().map(|value| value.nonce.clone())),
        relay_ca_key_version: Set(input.relay_ca.as_ref().map(|value| value.key_version)),
        client_cert_ciphertext: Set(input
            .client_cert
            .as_ref()
            .map(|value| value.ciphertext.clone())),
        client_cert_nonce: Set(input.client_cert.as_ref().map(|value| value.nonce.clone())),
        client_cert_key_version: Set(input.client_cert.as_ref().map(|value| value.key_version)),
        client_key_ciphertext: Set(input
            .client_key
            .as_ref()
            .map(|value| value.ciphertext.clone())),
        client_key_nonce: Set(input.client_key.as_ref().map(|value| value.nonce.clone())),
        client_key_key_version: Set(input.client_key.as_ref().map(|value| value.key_version)),
        bridge_encryption_key_ciphertext: Set(input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.ciphertext.clone())),
        bridge_encryption_key_nonce: Set(input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.nonce.clone())),
        bridge_encryption_key_key_version: Set(input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.key_version)),
        ..Default::default()
    }
}

pub async fn list_managed_relays(pool: &PgPool) -> Result<Vec<ManagedRelayRow>> {
    let db = postgres_connection(pool);
    let rows = managed_relays::Entity::find()
        .order_by(Expr::cust("lower(name)"), Order::Asc)
        .order_by_asc(managed_relays::Column::CreatedAt)
        .order_by_asc(managed_relays::Column::RelayId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(row_from_model).collect())
}

pub async fn list_managed_relays_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<(i64, i64, Vec<ManagedRelayRow>)> {
    let db = postgres_connection(pool);
    let counts = sea_orm::sea_query::Query::select()
        .expr_as(
            Expr::col(managed_relays::Column::RelayId).count(),
            Alias::new("total"),
        )
        .expr_as(
            Expr::cust("COUNT(*) FILTER (WHERE enabled)"),
            Alias::new("enabled_count"),
        )
        .from(managed_relays::Entity)
        .to_owned();
    let count_row = db
        .query_one(&counts)
        .await
        .map_err(native_error)?
        .ok_or_else(|| anyhow::anyhow!("managed relay counts returned no row"))?;
    let total: i64 = count_row.try_get("", "total").map_err(native_error)?;
    let enabled_count: i64 = count_row
        .try_get("", "enabled_count")
        .map_err(native_error)?;

    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let relays = managed_relays::Entity::find()
        .order_by(Expr::cust("lower(name)"), Order::Asc)
        .order_by_asc(managed_relays::Column::CreatedAt)
        .order_by_asc(managed_relays::Column::RelayId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((
        total,
        enabled_count,
        relays.into_iter().map(row_from_model).collect(),
    ))
}

pub async fn list_enabled_managed_relays(pool: &PgPool) -> Result<Vec<ManagedRelayRow>> {
    let db = postgres_connection(pool);
    let rows = managed_relays::Entity::find()
        .filter(managed_relays::Column::Enabled.eq(true))
        .order_by(Expr::cust("lower(name)"), Order::Asc)
        .order_by_asc(managed_relays::Column::CreatedAt)
        .order_by_asc(managed_relays::Column::RelayId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(row_from_model).collect())
}

pub async fn get_managed_relay(pool: &PgPool, relay_id: Uuid) -> Result<Option<ManagedRelayRow>> {
    let db = postgres_connection(pool);
    let row = managed_relays::Entity::find_by_id(relay_id)
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.map(row_from_model))
}

pub async fn create_managed_relay(
    pool: &PgPool,
    input: ManagedRelayInput,
) -> Result<ManagedRelayRow> {
    let db = postgres_connection(pool);
    let mut model = active_model(input);
    model.relay_id = Set(Uuid::new_v4());
    let row = managed_relays::Entity::insert(model)
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
    Ok(row_from_model(row))
}

pub async fn update_managed_relay(
    pool: &PgPool,
    relay_id: Uuid,
    input: ManagedRelayInput,
) -> Result<Option<ManagedRelayRow>> {
    let db = postgres_connection(pool);
    let rows = managed_relays::Entity::update_many()
        .set(active_model(input))
        .col_expr(managed_relays::Column::UpdatedAt, Expr::current_timestamp())
        .filter(managed_relays::Column::RelayId.eq(relay_id))
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().next().map(row_from_model))
}

pub async fn delete_managed_relay(pool: &PgPool, relay_id: Uuid) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = managed_relays::Entity::delete_many()
        .filter(managed_relays::Column::RelayId.eq(relay_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}
