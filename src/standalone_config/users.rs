use anyhow::{Result, anyhow};
use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use sqlx::SqlitePool;

use crate::{
    db::entities::sqlite::standalone_users::{self, ActiveModel, Column, Entity},
    db::seaorm::{native_error, sqlite_connection},
    db::{User, UserCreate, UserPassword, UserUpdate},
    keys::hash_password,
};

#[derive(Clone)]
pub struct SqliteUserStore {
    pool: SqlitePool,
}

impl SqliteUserStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create the first admin when needed. An empty `password` generates a
    /// strong random one; existing logins and existing active users are never
    /// modified.
    pub(crate) async fn bootstrap_admin(
        &self,
        login: &str,
        password: &str,
    ) -> Result<Option<String>> {
        let mut captured: Option<String> = None;
        self.ensure_bootstrap_admin(
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

    /// See [`Self::bootstrap_admin`]. The generated candidate is handed to
    /// `resolve_generated` BEFORE the admin row is committed, and the
    /// returned effective password is what gets stored. This supports atomic
    /// create-if-absent publication across racing starters: if another
    /// process already published a password file, the resolver returns that
    /// existing password so every process inserts the same value. A failing
    /// resolver aborts without leaving an active account whose password
    /// cannot be retrieved on a later start.
    pub(crate) async fn ensure_bootstrap_admin(
        &self,
        login: &str,
        password: Option<&str>,
        resolve_generated: impl FnOnce(&str) -> Result<String>,
    ) -> Result<()> {
        let db = sqlite_connection(&self.pool);
        let login_exists = Entity::find()
            .select_only()
            .column(Column::UserId)
            .filter(Column::LoginName.eq(login))
            .limit(1)
            .into_tuple::<i64>()
            .one(&db)
            .await
            .map_err(native_error)?
            .is_some();
        if login_exists {
            return Ok(());
        }

        let has_active_user = Entity::find()
            .select_only()
            .column(Column::UserId)
            .filter(Column::Enabled.eq(1))
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
        Entity::insert(ActiveModel {
            login_name: Set(login.trim().to_string()),
            password_hash: Set(password_hash),
            display_name: Set(login.trim().to_string()),
            is_admin: Set(1),
            enabled: Set(1),
            ..Default::default()
        })
        .exec(&db)
        .await
        .map_err(native_error)?;
        Ok(())
    }

    pub(crate) async fn get_user_password_by_login(
        &self,
        login: &str,
    ) -> Result<Option<UserPassword>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find()
            .filter(Column::LoginName.eq(login))
            .one(&db)
            .await
            .map_err(native_error)?;
        model.map(user_password_from_model).transpose()
    }

    pub(crate) async fn get_active_user(&self, user_id: i64) -> Result<Option<User>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Enabled.eq(1))
            .one(&db)
            .await
            .map_err(native_error)?;
        model.map(user_from_model).transpose()
    }

    pub(crate) async fn list_users(&self) -> Result<Vec<User>> {
        let db = sqlite_connection(&self.pool);
        let models = Entity::find()
            .order_by_desc(Column::UpdatedAt)
            .order_by_desc(Column::UserId)
            .all(&db)
            .await
            .map_err(native_error)?;
        models.into_iter().map(user_from_model).collect()
    }

    pub(crate) async fn list_users_page(&self, first: i64, rows: i64) -> Result<(i64, Vec<User>)> {
        let db = sqlite_connection(&self.pool);
        let total = Entity::find().count(&db).await.map_err(native_error)? as i64;
        let models = Entity::find()
            .order_by_desc(Column::UpdatedAt)
            .order_by_desc(Column::UserId)
            .limit(rows.clamp(1, 200) as u64)
            .offset(first.max(0) as u64)
            .all(&db)
            .await
            .map_err(native_error)?;
        Ok((
            total,
            models
                .into_iter()
                .map(user_from_model)
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    pub(crate) async fn create_user(&self, input: UserCreate) -> Result<User> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::insert(ActiveModel {
            login_name: Set(input.login_name),
            password_hash: Set(input.password_hash),
            display_name: Set(input.display_name),
            is_admin: Set(i32::from(input.is_admin)),
            enabled: Set(1),
            ..Default::default()
        })
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?;
        user_from_model(model)
    }

    pub(crate) async fn update_user(
        &self,
        user_id: i64,
        input: UserUpdate,
    ) -> Result<Option<User>> {
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(native_error)?;
        let mut changes = <ActiveModel as ActiveModelTrait>::default();
        if let Some(display_name) = input.display_name {
            changes.display_name = Set(display_name);
        }
        if let Some(is_admin) = input.is_admin {
            changes.is_admin = Set(i32::from(is_admin));
        }
        if let Some(is_active) = input.is_active {
            changes.enabled = Set(i32::from(is_active));
        }
        let updated = match Entity::update_many()
            .set(changes)
            .col_expr(Column::UpdatedAt, sqlite_updated_at())
            .filter(Column::UserId.eq(user_id))
            .exec(&transaction)
            .await
        {
            Ok(updated) => updated,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(native_error(error));
            }
        };
        if updated.rows_affected == 0 {
            let _ = transaction.rollback().await;
            return Ok(None);
        }
        let model = match Entity::find()
            .filter(Column::UserId.eq(user_id))
            .one(&transaction)
            .await
        {
            Ok(model) => model,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(native_error(error));
            }
        };
        let user = model.map(user_from_model).transpose();
        transaction.commit().await.map_err(native_error)?;
        user
    }

    pub(crate) async fn reset_password(&self, user_id: i64, password_hash: String) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::update_many()
            .col_expr(Column::PasswordHash, Expr::value(password_hash))
            .col_expr(Column::UpdatedAt, sqlite_updated_at())
            .filter(Column::UserId.eq(user_id))
            .exec(&db)
            .await
            .map_err(native_error)?;
        Ok(result.rows_affected > 0)
    }

    pub(crate) async fn delete_user(&self, user_id: i64) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::delete_many()
            .filter(Column::UserId.eq(user_id))
            .exec(&db)
            .await
            .map_err(native_error)?;
        Ok(result.rows_affected > 0)
    }
}

fn user_from_model(model: standalone_users::Model) -> Result<User> {
    Ok(User {
        user_id: model.user_id,
        login_name: model.login_name,
        display_name: model.display_name,
        is_admin: sqlite_flag(model.is_admin, "is_admin")?,
        is_active: sqlite_flag(model.enabled, "enabled")?,
        created_at: sqlite_timestamp(&model.created_at, "created_at")?,
        updated_at: sqlite_timestamp(&model.updated_at, "updated_at")?,
    })
}

fn user_password_from_model(model: standalone_users::Model) -> Result<UserPassword> {
    Ok(UserPassword {
        user_id: model.user_id,
        login_name: model.login_name,
        password_hash: model.password_hash,
        display_name: model.display_name,
        is_admin: sqlite_flag(model.is_admin, "is_admin")?,
        is_active: sqlite_flag(model.enabled, "enabled")?,
    })
}

fn sqlite_flag(value: i32, column: &str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(anyhow!(
            "SQLite user column {column} is not boolean: {value}"
        )),
    }
}

fn sqlite_updated_at() -> Expr {
    Expr::cust("strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
}

fn sqlite_timestamp(value: &str, column: &str) -> Result<DateTime<Utc>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&Utc));
    }
    let timestamp = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .map_err(|error| anyhow!("SQLite user column {column} is not a timestamp: {error}"))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_model(user_id: i64, is_admin: i32, enabled: i32) -> standalone_users::Model {
        standalone_users::Model {
            user_id,
            login_name: "probe".to_string(),
            display_name: "Probe".to_string(),
            password_hash: "hash".to_string(),
            is_admin,
            enabled,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:01".to_string(),
        }
    }

    #[test]
    fn projection_preserves_full_i64_user_id_and_flags() {
        let user_id = i64::MAX - 11;
        let user = user_from_model(user_model(user_id, 1, 0)).expect("project the user row");
        assert_eq!(user.user_id, user_id);
        assert!(user.is_admin);
        assert!(!user.is_active);
    }

    #[test]
    fn projection_rejects_non_boolean_flags() {
        let error = user_from_model(user_model(1, 2, 0)).expect_err("reject non-boolean is_admin");
        assert!(error.to_string().contains("is_admin"));
        let error = user_from_model(user_model(1, 0, 3)).expect_err("reject non-boolean enabled");
        assert!(error.to_string().contains("enabled"));
    }
}
