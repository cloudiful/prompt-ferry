use anyhow::{Result, anyhow};
use sqlx::PgPool;

use crate::{
    db::types::{ClientKey, ClientKeyIdentity, User, UserCreate, UserPassword, UserUpdate},
    keys::hash_password,
    standalone_config::SqliteUserStore,
};

#[derive(Clone)]
pub enum UserStore {
    Postgres(PgPool),
    Sqlite(SqliteUserStore),
}

impl UserStore {
    pub fn postgres(pool: &PgPool) -> Self {
        Self::Postgres(pool.clone())
    }

    pub fn sqlite(pool: sqlx::SqlitePool) -> Self {
        Self::Sqlite(SqliteUserStore::new(pool))
    }

    pub fn is_sqlite(&self) -> bool {
        matches!(self, Self::Sqlite(_))
    }

    pub async fn bootstrap_admin(&self, login: &str, password: &str) -> Result<()> {
        match self {
            Self::Postgres(pool) => bootstrap_admin(pool, login, password).await,
            Self::Sqlite(store) => store.bootstrap_admin(login, password).await,
        }
    }

    pub async fn get_user_password_by_login(&self, login: &str) -> Result<Option<UserPassword>> {
        match self {
            Self::Postgres(pool) => get_user_password_by_login(pool, login).await,
            Self::Sqlite(store) => store.get_user_password_by_login(login).await,
        }
    }

    pub async fn get_active_user(&self, user_id: i64) -> Result<Option<User>> {
        match self {
            Self::Postgres(pool) => get_active_user(pool, user_id).await,
            Self::Sqlite(store) => store.get_active_user(user_id).await,
        }
    }

    pub async fn list_users(&self) -> Result<Vec<User>> {
        match self {
            Self::Postgres(pool) => list_users(pool).await,
            Self::Sqlite(store) => store.list_users().await,
        }
    }

    pub async fn list_users_page(&self, first: i64, rows: i64) -> Result<(i64, Vec<User>)> {
        match self {
            Self::Postgres(pool) => list_users_page(pool, first, rows).await,
            Self::Sqlite(store) => store.list_users_page(first, rows).await,
        }
    }

    pub async fn create_user(&self, input: UserCreate) -> Result<User> {
        match self {
            Self::Postgres(pool) => create_user(pool, input).await,
            Self::Sqlite(store) => store.create_user(input).await,
        }
    }

    pub async fn update_user(&self, user_id: i64, input: UserUpdate) -> Result<Option<User>> {
        match self {
            Self::Postgres(pool) => update_user(pool, user_id, input).await,
            Self::Sqlite(store) => store.update_user(user_id, input).await,
        }
    }

    pub async fn reset_password(&self, user_id: i64, password_hash: String) -> Result<bool> {
        match self {
            Self::Postgres(pool) => reset_password(pool, user_id, password_hash).await,
            Self::Sqlite(store) => store.reset_password(user_id, password_hash).await,
        }
    }

    pub async fn delete_user(&self, user_id: i64) -> Result<bool> {
        match self {
            Self::Postgres(pool) => delete_user(pool, user_id).await,
            Self::Sqlite(store) => store.delete_user(user_id).await,
        }
    }
}

pub async fn bootstrap_admin(pool: &PgPool, login: &str, password: &str) -> Result<()> {
    let login_exists = sqlx::query_file!("src/sql/users/bootstrap_admin_exists.sql", login)
        .fetch_one(pool)
        .await?
        .exists;
    if login_exists {
        return Ok(());
    }

    let has_active_user = sqlx::query_file!("src/sql/users/has_active_users.sql")
        .fetch_one(pool)
        .await?
        .has_active;
    if has_active_user {
        return Ok(());
    }
    require_bootstrap_credentials(login, password)?;
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

fn require_bootstrap_credentials(login: &str, password: &str) -> Result<()> {
    if login.trim().is_empty() || password.trim().is_empty() {
        return Err(anyhow!(
            "bootstrap admin login and password are required when no active admin exists"
        ));
    }
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

pub async fn list_users_page(pool: &PgPool, first: i64, rows: i64) -> Result<(i64, Vec<User>)> {
    let total = sqlx::query_file!("src/sql/users/count_users.sql")
        .fetch_one(pool)
        .await?
        .total;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let users = sqlx::query_file_as!(User, "src/sql/users/list_users_page.sql", first, rows,)
        .fetch_all(pool)
        .await?;
    Ok((total, users))
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
        sqlx::query_file_as!(ClientKey, "src/sql/users/list_client_keys_all.sql", user_id,)
            .fetch_all(pool)
            .await?,
    )
}

pub async fn list_client_keys_page(
    pool: &PgPool,
    user_id: i64,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<ClientKey>)> {
    let total = count_client_keys(pool, user_id).await?;
    let first = first.max(0);
    let rows = rows.clamp(1, 200);
    let keys = sqlx::query_file_as!(
        ClientKey,
        "src/sql/users/list_client_keys.sql",
        user_id,
        first,
        rows,
    )
    .fetch_all(pool)
    .await?;
    Ok((total, keys))
}

pub async fn get_client_key_label_by_hash(pool: &PgPool, key_hash: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_file_scalar!("src/sql/users/get_client_key_label_by_hash.sql", key_hash,)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn get_client_key_identity_by_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<ClientKeyIdentity>> {
    Ok(sqlx::query_file_as!(
        ClientKeyIdentity,
        "src/sql/users/get_client_key_identity_by_hash.sql",
        key_hash,
    )
    .fetch_optional(pool)
    .await?)
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
