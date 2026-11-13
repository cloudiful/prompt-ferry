use anyhow::{Result, anyhow};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use sqlx::PgPool;

use crate::{
    db::entities::postgres::{client_keys, users},
    db::seaorm::{native_error, postgres_connection},
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

    pub async fn bootstrap_admin(&self, login: &str, password: &str) -> Result<Option<String>> {
        match self {
            Self::Postgres(pool) => bootstrap_admin(pool, login, password).await,
            Self::Sqlite(store) => store.bootstrap_admin(login, password).await,
        }
    }

    /// See [`ensure_bootstrap_admin`]: the resolver returns the effective
    /// generated password (reusing an existing publication when another
    /// starter won) and it is committed to the database.
    pub async fn ensure_bootstrap_admin(
        &self,
        login: &str,
        password: Option<&str>,
        resolve_generated: impl FnOnce(&str) -> Result<String>,
    ) -> Result<()> {
        match self {
            Self::Postgres(pool) => {
                ensure_bootstrap_admin(pool, login, password, resolve_generated).await
            }
            Self::Sqlite(store) => {
                store
                    .ensure_bootstrap_admin(login, password, resolve_generated)
                    .await
            }
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

fn user_from_model(model: users::Model) -> User {
    User {
        user_id: model.user_id,
        login_name: model.login_name,
        display_name: model.display_name,
        is_admin: model.is_admin,
        is_active: model.is_active,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn client_key_from_model(model: client_keys::Model) -> ClientKey {
    ClientKey {
        key_id: model.key_id,
        user_id: model.user_id,
        key_prefix: model.key_prefix,
        label: model.label,
        enabled: model.enabled,
        last_used_at: model.last_used_at,
        created_at: model.created_at,
        secret: model.secret,
    }
}

pub async fn bootstrap_admin(pool: &PgPool, login: &str, password: &str) -> Result<Option<String>> {
    let mut captured: Option<String> = None;
    ensure_bootstrap_admin(
        pool,
        login,
        (!password.trim().is_empty()).then_some(password),
        |generated| {
            captured = Some(generated.to_string());
            Ok(generated.to_string())
        },
    )
    .await?;
    Ok(captured)
}

/// Create the first admin only when needed: the login does not exist yet and
/// no active user exists. An empty `password` generates a strong random one.
///
/// `resolve_generated` is invoked with the generated candidate password
/// BEFORE the admin row is committed and must return the effective password
/// to store. This supports atomic create-if-absent publication: if another
/// starter already published a password file, the resolver returns that
/// existing password so every racing process inserts the same value. A
/// failing resolver aborts without leaving an active account whose password
/// cannot be retrieved. It is not called for configured passwords.
pub async fn ensure_bootstrap_admin(
    pool: &PgPool,
    login: &str,
    password: Option<&str>,
    resolve_generated: impl FnOnce(&str) -> Result<String>,
) -> Result<()> {
    let db = postgres_connection(pool);

    let login_exists = users::Entity::find()
        .select_only()
        .column(users::Column::UserId)
        .filter(users::Column::LoginName.eq(login))
        .limit(1)
        .into_tuple::<i64>()
        .one(&db)
        .await
        .map_err(native_error)?
        .is_some();
    if login_exists {
        return Ok(());
    }

    let has_active_user = users::Entity::find()
        .select_only()
        .column(users::Column::UserId)
        .filter(users::Column::IsActive.eq(true))
        .limit(1)
        .into_tuple::<i64>()
        .one(&db)
        .await
        .map_err(native_error)?
        .is_some();
    if has_active_user {
        return Ok(());
    }
    if login.trim().is_empty() {
        return Err(anyhow!(
            "bootstrap admin login must not be empty or whitespace when no active admin exists"
        ));
    }
    // An empty configured password is allowed on first startup only: generate
    // a candidate, let the resolver atomically publish or reuse an existing
    // publication, and commit the effective password it returns.
    let generated = password.map(str::trim).unwrap_or("").is_empty();
    let candidate = if generated {
        crate::relay_secrets::generate_bootstrap_password()
    } else {
        password.unwrap_or_default().to_string()
    };
    let effective = if generated {
        resolve_generated(&candidate)?
    } else {
        candidate
    };
    let password_hash = hash_password(&effective)?;
    users::Entity::insert(users::ActiveModel {
        login_name: Set(login.to_string()),
        password_hash: Set(password_hash),
        display_name: Set(login.to_string()),
        is_admin: Set(true),
        ..Default::default()
    })
    .exec(&db)
    .await
    .map_err(native_error)?;
    Ok(())
}

pub async fn get_user_password_by_login(
    pool: &PgPool,
    login: &str,
) -> Result<Option<UserPassword>> {
    let db = postgres_connection(pool);
    let user = users::Entity::find()
        .filter(users::Column::LoginName.eq(login))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(user.map(|model| UserPassword {
        user_id: model.user_id,
        login_name: model.login_name,
        password_hash: model.password_hash,
        display_name: model.display_name,
        is_admin: model.is_admin,
        is_active: model.is_active,
    }))
}

pub async fn get_active_user(pool: &PgPool, user_id: i64) -> Result<Option<User>> {
    let db = postgres_connection(pool);
    let user = users::Entity::find()
        .filter(users::Column::UserId.eq(user_id))
        .filter(users::Column::IsActive.eq(true))
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(user.map(user_from_model))
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<User>> {
    let db = postgres_connection(pool);
    let users = users::Entity::find()
        .order_by_desc(users::Column::UpdatedAt)
        .order_by_desc(users::Column::UserId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(users.into_iter().map(user_from_model).collect())
}

pub async fn list_users_page(pool: &PgPool, first: i64, rows: i64) -> Result<(i64, Vec<User>)> {
    let db = postgres_connection(pool);
    let total = users::Entity::find()
        .count(&db)
        .await
        .map_err(native_error)? as i64;
    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let users = users::Entity::find()
        .order_by_desc(users::Column::UpdatedAt)
        .order_by_desc(users::Column::UserId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((total, users.into_iter().map(user_from_model).collect()))
}

pub async fn create_user(pool: &PgPool, input: UserCreate) -> Result<User> {
    let db = postgres_connection(pool);
    let user = users::Entity::insert(users::ActiveModel {
        login_name: Set(input.login_name),
        password_hash: Set(input.password_hash),
        display_name: Set(input.display_name),
        is_admin: Set(input.is_admin),
        ..Default::default()
    })
    .exec_with_returning(&db)
    .await
    .map_err(native_error)?;
    Ok(user_from_model(user))
}

pub async fn update_user(pool: &PgPool, user_id: i64, input: UserUpdate) -> Result<Option<User>> {
    let db = postgres_connection(pool);
    let mut changes = <users::ActiveModel as ActiveModelTrait>::default();
    if let Some(display_name) = input.display_name {
        changes.display_name = Set(display_name);
    }
    if let Some(is_admin) = input.is_admin {
        changes.is_admin = Set(is_admin);
    }
    if let Some(is_active) = input.is_active {
        changes.is_active = Set(is_active);
    }
    let user = users::Entity::update_many()
        .set(changes)
        .col_expr(users::Column::UpdatedAt, Expr::current_timestamp())
        .filter(users::Column::UserId.eq(user_id))
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
    Ok(user.into_iter().next().map(user_from_model))
}

pub async fn reset_password(pool: &PgPool, user_id: i64, password_hash: String) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = users::Entity::update_many()
        .col_expr(users::Column::PasswordHash, Expr::value(password_hash))
        .col_expr(users::Column::UpdatedAt, Expr::current_timestamp())
        .filter(users::Column::UserId.eq(user_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

pub async fn delete_user(pool: &PgPool, user_id: i64) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = users::Entity::delete_many()
        .filter(users::Column::UserId.eq(user_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

pub async fn create_client_key(
    pool: &PgPool,
    user_id: i64,
    label: &str,
    key_prefix: &str,
    key_hash: &str,
    secret: &str,
) -> Result<ClientKey> {
    let db = postgres_connection(pool);
    let key = client_keys::Entity::insert(client_keys::ActiveModel {
        user_id: Set(user_id),
        label: Set(label.to_string()),
        key_prefix: Set(key_prefix.to_string()),
        key_hash: Set(key_hash.to_string()),
        secret: Set(Some(secret.to_string())),
        ..Default::default()
    })
    .exec_with_returning(&db)
    .await
    .map_err(native_error)?;
    Ok(client_key_from_model(key))
}

pub async fn count_client_keys(pool: &PgPool, user_id: i64) -> Result<i64> {
    let db = postgres_connection(pool);
    let total = client_keys::Entity::find()
        .filter(client_keys::Column::UserId.eq(user_id))
        .count(&db)
        .await
        .map_err(native_error)?;
    Ok(total as i64)
}

pub async fn list_client_keys(pool: &PgPool, user_id: i64) -> Result<Vec<ClientKey>> {
    let db = postgres_connection(pool);
    let keys = client_keys::Entity::find()
        .filter(client_keys::Column::UserId.eq(user_id))
        .order_by_desc(client_keys::Column::CreatedAt)
        .order_by_desc(client_keys::Column::KeyId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(keys.into_iter().map(client_key_from_model).collect())
}

pub async fn list_client_keys_page(
    pool: &PgPool,
    user_id: i64,
    first: i64,
    rows: i64,
) -> Result<(i64, Vec<ClientKey>)> {
    let total = count_client_keys(pool, user_id).await?;
    let db = postgres_connection(pool);
    let first = std::cmp::Ord::max(first, 0);
    let rows = rows.clamp(1, 200);
    let keys = client_keys::Entity::find()
        .filter(client_keys::Column::UserId.eq(user_id))
        .order_by_desc(client_keys::Column::CreatedAt)
        .order_by_desc(client_keys::Column::KeyId)
        .limit(rows as u64)
        .offset(first as u64)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok((total, keys.into_iter().map(client_key_from_model).collect()))
}

pub async fn get_client_key_label_by_hash(pool: &PgPool, key_hash: &str) -> Result<Option<String>> {
    let db = postgres_connection(pool);
    Ok(client_keys::Entity::find()
        .select_only()
        .column(client_keys::Column::Label)
        .filter(client_keys::Column::KeyHash.eq(key_hash))
        .limit(1)
        .into_tuple::<String>()
        .one(&db)
        .await
        .map_err(native_error)?)
}

pub async fn get_client_key_identity_by_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<ClientKeyIdentity>> {
    let db = postgres_connection(pool);
    let identity = client_keys::Entity::find()
        .select_only()
        .column(client_keys::Column::KeyId)
        .column(client_keys::Column::Label)
        .filter(client_keys::Column::KeyHash.eq(key_hash))
        .limit(1)
        .into_tuple::<(i64, String)>()
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(identity.map(|(key_id, label)| ClientKeyIdentity { key_id, label }))
}

pub async fn update_client_key(
    pool: &PgPool,
    user_id: i64,
    key_id: i64,
    label: Option<String>,
    enabled: Option<bool>,
) -> Result<Option<ClientKey>> {
    let db = postgres_connection(pool);
    if label.is_none() && enabled.is_none() {
        let key = client_keys::Entity::find()
            .filter(client_keys::Column::UserId.eq(user_id))
            .filter(client_keys::Column::KeyId.eq(key_id))
            .one(&db)
            .await
            .map_err(native_error)?;
        return Ok(key.map(client_key_from_model));
    }

    let mut changes = <client_keys::ActiveModel as ActiveModelTrait>::default();
    if let Some(label) = label {
        changes.label = Set(label);
    }
    if let Some(enabled) = enabled {
        changes.enabled = Set(enabled);
    }
    let key = client_keys::Entity::update_many()
        .set(changes)
        .filter(client_keys::Column::UserId.eq(user_id))
        .filter(client_keys::Column::KeyId.eq(key_id))
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
    Ok(key.into_iter().next().map(client_key_from_model))
}

pub async fn delete_client_key(pool: &PgPool, user_id: i64, key_id: i64) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = client_keys::Entity::delete_many()
        .filter(client_keys::Column::UserId.eq(user_id))
        .filter(client_keys::Column::KeyId.eq(key_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

pub async fn get_user_endpoint_setting(pool: &PgPool, user_id: i64) -> Result<Option<uuid::Uuid>> {
    Ok(
        sqlx::query_file_scalar!("src/sql/users/get_user_endpoint_setting.sql", user_id,)
            .fetch_optional(pool)
            .await?
            .flatten(),
    )
}
