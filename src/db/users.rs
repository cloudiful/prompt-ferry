use anyhow::Result;
use sqlx::PgPool;

use crate::{
    db::types::{ClientKey, User, UserCreate, UserPassword, UserUpdate},
    keys::hash_password,
};

pub async fn bootstrap_admin(pool: &PgPool, login: &str, password: &str) -> Result<()> {
    if login.trim().is_empty() || password.trim().is_empty() {
        return Ok(());
    }
    let exists = sqlx::query_file!("src/sql/users/bootstrap_admin_exists.sql", login)
        .fetch_one(pool)
        .await?
        .exists;
    if exists {
        return Ok(());
    }
    let password_hash = hash_password(password)?;
    sqlx::query_file!(
        "src/sql/users/bootstrap_admin_insert.sql",
        login,
        password_hash,
        login,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_user_password_by_login(
    pool: &PgPool,
    login: &str,
) -> Result<Option<UserPassword>> {
    Ok(sqlx::query_file_as!(
        UserPassword,
        "src/sql/users/get_user_password_by_login.sql",
        login,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_active_user(pool: &PgPool, user_id: i64) -> Result<Option<User>> {
    Ok(
        sqlx::query_file_as!(User, "src/sql/users/get_active_user.sql", user_id,)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<User>> {
    Ok(sqlx::query_file_as!(User, "src/sql/users/list_users.sql",)
        .fetch_all(pool)
        .await?)
}

pub async fn create_user(pool: &PgPool, input: UserCreate) -> Result<User> {
    Ok(sqlx::query_file_as!(
        User,
        "src/sql/users/create_user.sql",
        input.login_name,
        input.password_hash,
        input.display_name,
        input.is_admin,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_user(pool: &PgPool, user_id: i64, input: UserUpdate) -> Result<Option<User>> {
    Ok(sqlx::query_file_as!(
        User,
        "src/sql/users/update_user.sql",
        user_id,
        input.display_name,
        input.is_admin,
        input.is_active,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn reset_password(pool: &PgPool, user_id: i64, password_hash: String) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/users/reset_password.sql", user_id, password_hash,)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_user(pool: &PgPool, user_id: i64) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/users/delete_user.sql", user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn create_client_key(
    pool: &PgPool,
    user_id: i64,
    label: &str,
    key_prefix: &str,
    key_hash: &str,
    secret: &str,
) -> Result<ClientKey> {
    Ok(sqlx::query_file_as!(
        ClientKey,
        "src/sql/users/create_client_key.sql",
        user_id,
        label,
        key_prefix,
        key_hash,
        secret,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn count_client_keys(pool: &PgPool, user_id: i64) -> Result<i64> {
    Ok(
        sqlx::query_file_scalar!("src/sql/users/count_client_keys.sql", user_id,)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn list_client_keys(pool: &PgPool, user_id: i64) -> Result<Vec<ClientKey>> {
    Ok(
        sqlx::query_file_as!(ClientKey, "src/sql/users/list_client_keys.sql", user_id,)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn get_client_key_label_by_hash(pool: &PgPool, key_hash: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_file_scalar!("src/sql/users/get_client_key_label_by_hash.sql", key_hash,)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn update_client_key(
    pool: &PgPool,
    user_id: i64,
    key_id: i64,
    label: Option<String>,
    enabled: Option<bool>,
) -> Result<Option<ClientKey>> {
    Ok(sqlx::query_file_as!(
        ClientKey,
        "src/sql/users/update_client_key.sql",
        user_id,
        key_id,
        label,
        enabled,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_client_key(pool: &PgPool, user_id: i64, key_id: i64) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/users/delete_client_key.sql", user_id, key_id,)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_user_endpoint_setting(pool: &PgPool, user_id: i64) -> Result<Option<uuid::Uuid>> {
    Ok(
        sqlx::query_file_scalar!("src/sql/users/get_user_endpoint_setting.sql", user_id,)
            .fetch_optional(pool)
            .await?
            .flatten(),
    )
}
