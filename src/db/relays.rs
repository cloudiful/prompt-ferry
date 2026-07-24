use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{ManagedRelayInput, ManagedRelayRow};

pub async fn list_managed_relays(pool: &PgPool) -> Result<Vec<ManagedRelayRow>> {
    Ok(
        sqlx::query_file_as!(ManagedRelayRow, "src/sql/relays/list_managed_relays.sql",)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn list_managed_relays_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<(i64, i64, Vec<ManagedRelayRow>)> {
    let counts = sqlx::query_file!("src/sql/relays/count_managed_relays.sql")
        .fetch_one(pool)
        .await?;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let relays = sqlx::query_file_as!(
        ManagedRelayRow,
        "src/sql/relays/list_managed_relays_page.sql",
        first,
        rows,
    )
    .fetch_all(pool)
    .await?;
    Ok((counts.total, counts.enabled_count, relays))
}

pub async fn list_enabled_managed_relays(pool: &PgPool) -> Result<Vec<ManagedRelayRow>> {
    Ok(sqlx::query_file_as!(
        ManagedRelayRow,
        "src/sql/relays/list_enabled_managed_relays.sql",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_managed_relay(pool: &PgPool, relay_id: Uuid) -> Result<Option<ManagedRelayRow>> {
    Ok(sqlx::query_file_as!(
        ManagedRelayRow,
        "src/sql/relays/get_managed_relay.sql",
        relay_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn create_managed_relay(
    pool: &PgPool,
    input: ManagedRelayInput,
) -> Result<ManagedRelayRow> {
    Ok(sqlx::query_file_as!(
        ManagedRelayRow,
        "src/sql/relays/create_managed_relay.sql",
        Uuid::new_v4(),
        input.name,
        input.relay_url,
        input.enabled,
        input.tls_mode.as_str(),
        input.bridge_encryption_mode.as_str(),
        input
            .relay_ca
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.relay_ca.as_ref().map(|value| value.nonce.clone()),
        input.relay_ca.as_ref().map(|value| value.key_version),
        input
            .client_cert
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.client_cert.as_ref().map(|value| value.nonce.clone()),
        input.client_cert.as_ref().map(|value| value.key_version),
        input
            .client_key
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.client_key.as_ref().map(|value| value.nonce.clone()),
        input.client_key.as_ref().map(|value| value.key_version),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.nonce.clone()),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.key_version),
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_managed_relay(
    pool: &PgPool,
    relay_id: Uuid,
    input: ManagedRelayInput,
) -> Result<Option<ManagedRelayRow>> {
    Ok(sqlx::query_file_as!(
        ManagedRelayRow,
        "src/sql/relays/update_managed_relay.sql",
        relay_id,
        input.name,
        input.relay_url,
        input.enabled,
        input.tls_mode.as_str(),
        input.bridge_encryption_mode.as_str(),
        input
            .relay_ca
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.relay_ca.as_ref().map(|value| value.nonce.clone()),
        input.relay_ca.as_ref().map(|value| value.key_version),
        input
            .client_cert
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.client_cert.as_ref().map(|value| value.nonce.clone()),
        input.client_cert.as_ref().map(|value| value.key_version),
        input
            .client_key
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input.client_key.as_ref().map(|value| value.nonce.clone()),
        input.client_key.as_ref().map(|value| value.key_version),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.ciphertext.clone()),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.nonce.clone()),
        input
            .bridge_encryption_key
            .as_ref()
            .map(|value| value.key_version),
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_managed_relay(pool: &PgPool, relay_id: Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/relays/delete_managed_relay.sql", relay_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
