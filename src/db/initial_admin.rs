use anyhow::Result;
use sea_orm::sea_query::{Expr, Query};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, EntityTrait, PaginatorTrait, Set, TransactionTrait,
};
use sqlx::PgPool;

use super::entities::postgres::{users, worker_settings};
use super::seaorm::{native_error, postgres_connection};

const MARKER: &str = "node_admin_initialized";
const LOCK: i64 = 0x5066363453657475;

async fn lock(tx: &sea_orm::DatabaseTransaction) -> Result<()> {
    let query = Query::select()
        .expr(Expr::cust_with_values("pg_advisory_xact_lock($1)", [LOCK]))
        .to_owned();
    tx.execute(&query).await.map_err(native_error)?;
    Ok(())
}

async fn marker_set<C: ConnectionTrait>(db: &C) -> Result<bool> {
    let marker = worker_settings::Entity::find_by_id(MARKER)
        .one(db)
        .await
        .map_err(native_error)?;
    Ok(marker.is_some_and(|row| row.setting_value == serde_json::json!(true)))
}

async fn has_users<C: ConnectionTrait>(db: &C) -> Result<bool> {
    Ok(users::Entity::find()
        .count(db)
        .await
        .map_err(native_error)?
        > 0)
}

fn setup_closed(marker: bool, any_user: bool) -> bool {
    marker || any_user
}

async fn initialized<C: ConnectionTrait>(db: &C) -> Result<bool> {
    let marker = marker_set(db).await?;
    Ok(setup_closed(
        marker,
        if marker { false } else { has_users(db).await? },
    ))
}

async fn set_marker<C: ConnectionTrait>(db: &C) -> Result<()> {
    use sea_orm::sea_query::OnConflict;
    worker_settings::Entity::insert(worker_settings::ActiveModel {
        setting_key: Set(MARKER.to_owned()),
        setting_value: Set(serde_json::json!(true)),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(worker_settings::Column::SettingKey)
            .update_column(worker_settings::Column::SettingValue)
            .to_owned(),
    )
    .exec(db)
    .await
    .map_err(native_error)?;
    Ok(())
}

pub async fn status(pool: &PgPool) -> Result<bool> {
    initialized(&postgres_connection(pool)).await
}

pub async fn close_for_existing_users(pool: &PgPool) -> Result<()> {
    let db = postgres_connection(pool);
    let tx = db.begin().await.map_err(native_error)?;
    lock(&tx).await?;
    if !marker_set(&tx).await? && has_users(&tx).await? {
        set_marker(&tx).await?;
    }
    tx.commit().await.map_err(native_error)?;
    Ok(())
}

pub async fn create(
    pool: &PgPool,
    login: &str,
    display: &str,
    password_hash: String,
) -> Result<bool> {
    let db = postgres_connection(pool);
    let tx = db.begin().await.map_err(native_error)?;
    lock(&tx).await?;
    if initialized(&tx).await? {
        return Ok(false);
    }
    users::ActiveModel {
        login_name: Set(login.to_owned()),
        display_name: Set(display.to_owned()),
        password_hash: Set(password_hash),
        is_admin: Set(true),
        is_active: Set(true),
        ..Default::default()
    }
    .insert(&tx)
    .await
    .map_err(native_error)?;
    set_marker(&tx).await?;
    tx.commit().await.map_err(native_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::setup_closed;

    #[test]
    fn only_an_unmarked_empty_database_is_open() {
        assert!(!setup_closed(false, false));
        assert!(setup_closed(false, true));
        assert!(setup_closed(true, true));
        assert!(setup_closed(true, false));
    }
}
