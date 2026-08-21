use anyhow::{Result, anyhow};
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use crate::{
    db::{User, UserCreate, UserPassword, UserUpdate},
    keys::hash_password,
};

macro_rules! sqlite_query {
    ($path:literal) => {
        sqlx::query(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/",
            $path
        )))
    };
}

#[derive(Clone)]
pub struct SqliteUserStore {
    pool: SqlitePool,
}

impl SqliteUserStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(crate) async fn bootstrap_admin(&self, login: &str, password: &str) -> Result<()> {
        let login_exists =
            sqlite_query!("src/standalone_config/sql/users/bootstrap_admin_exists.sql")
                .bind(login)
                .fetch_one(&self.pool)
                .await?
                .try_get::<i64, _>("exists")?
                != 0;
        if login_exists {
            return Ok(());
        }

        let has_active_user =
            sqlite_query!("src/standalone_config/sql/users/has_active_users_sqlite.sql")
                .fetch_one(&self.pool)
                .await?
                .try_get::<i64, _>("has_active")?
                != 0;
        if has_active_user {
            return Ok(());
        }
        if login.trim().is_empty() || password.trim().is_empty() {
            return Err(anyhow!(
                "bootstrap admin login and password are required when no active admin exists"
            ));
        }

        let password_hash = hash_password(password)?;
        sqlite_query!("src/standalone_config/sql/users/bootstrap_admin_insert.sql")
            .bind(login.trim())
            .bind(password_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn get_user_password_by_login(
        &self,
        login: &str,
    ) -> Result<Option<UserPassword>> {
        let row = sqlite_query!("src/standalone_config/sql/users/get_user_password_by_login.sql")
            .bind(login)
            .fetch_optional(&self.pool)
            .await?;
        row.map(sqlite_user_password).transpose()
    }

    pub(crate) async fn get_active_user(&self, user_id: i64) -> Result<Option<User>> {
        let row = sqlite_query!("src/standalone_config/sql/users/get_active_user.sql")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(sqlite_user).transpose()
    }

    pub(crate) async fn list_users(&self) -> Result<Vec<User>> {
        let rows = sqlite_query!("src/standalone_config/sql/users/list_users.sql")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(sqlite_user).collect()
    }

    pub(crate) async fn list_users_page(&self, first: i64, rows: i64) -> Result<(i64, Vec<User>)> {
        let total = sqlite_query!("src/standalone_config/sql/users/count_users.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let users = sqlite_query!("src/standalone_config/sql/users/list_users_page.sql")
            .bind(first.max(0))
            .bind(rows.clamp(1, 200))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(sqlite_user)
            .collect::<Result<Vec<_>>>()?;
        Ok((total, users))
    }

    pub(crate) async fn create_user(&self, input: UserCreate) -> Result<User> {
        let row = sqlite_query!("src/standalone_config/sql/users/create_user.sql")
            .bind(input.login_name)
            .bind(input.password_hash)
            .bind(input.display_name)
            .bind(input.is_admin)
            .fetch_one(&self.pool)
            .await?;
        sqlite_user(row)
    }

    pub(crate) async fn update_user(
        &self,
        user_id: i64,
        input: UserUpdate,
    ) -> Result<Option<User>> {
        let row = sqlite_query!("src/standalone_config/sql/users/update_user.sql")
            .bind(user_id)
            .bind(input.display_name)
            .bind(input.is_admin)
            .bind(input.is_active)
            .fetch_optional(&self.pool)
            .await?;
        row.map(sqlite_user).transpose()
    }

    pub(crate) async fn reset_password(&self, user_id: i64, password_hash: String) -> Result<bool> {
        let result = sqlite_query!("src/standalone_config/sql/users/reset_password.sql")
            .bind(user_id)
            .bind(password_hash)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub(crate) async fn delete_user(&self, user_id: i64) -> Result<bool> {
        let result = sqlite_query!("src/standalone_config/sql/users/delete_user.sql")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

fn sqlite_user_password(row: SqliteRow) -> Result<UserPassword> {
    Ok(UserPassword {
        user_id: row.try_get("user_id")?,
        login_name: row.try_get("login_name")?,
        password_hash: row.try_get("password_hash")?,
        display_name: row.try_get("display_name")?,
        is_admin: sqlite_bool(&row, "is_admin")?,
        is_active: sqlite_bool(&row, "is_active")?,
    })
}

fn sqlite_user(row: SqliteRow) -> Result<User> {
    Ok(User {
        user_id: row.try_get("user_id")?,
        login_name: row.try_get("login_name")?,
        display_name: row.try_get("display_name")?,
        is_admin: sqlite_bool(&row, "is_admin")?,
        is_active: sqlite_bool(&row, "is_active")?,
        created_at: sqlite_timestamp(&row, "created_at")?,
        updated_at: sqlite_timestamp(&row, "updated_at")?,
    })
}

fn sqlite_bool(row: &SqliteRow, column: &str) -> Result<bool> {
    match row.try_get::<i64, _>(column)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(anyhow!(
            "SQLite user column {column} is not boolean: {value}"
        )),
    }
}

fn sqlite_timestamp(row: &SqliteRow, column: &str) -> Result<DateTime<Utc>> {
    let value = row.try_get::<String, _>(column)?;
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(&value) {
        return Ok(timestamp.with_timezone(&Utc));
    }
    let timestamp = NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S")?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(timestamp, Utc))
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::SqliteUserStore;

    fn database_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-users-{suffix}.sqlite"))
    }

    #[tokio::test]
    async fn sqlite_bootstrap_requires_a_password_and_persists_auth_users() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        assert!(store.bootstrap_admin("admin", "").await.is_err());
        store
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap admin");
        store
            .bootstrap_admin("admin", "")
            .await
            .expect("existing admin is idempotent");

        let password = store
            .get_user_password_by_login("admin")
            .await
            .expect("load password")
            .expect("admin user");
        assert!(password.is_admin);
        assert!(password.is_active);
        assert!(crate::keys::verify_password(
            "correct horse battery staple",
            &password.password_hash
        ));

        let created = store
            .create_user(crate::db::UserCreate {
                login_name: "operator".to_string(),
                password_hash: crate::keys::hash_password("operator-password")
                    .expect("hash password"),
                display_name: "Operator".to_string(),
                is_admin: false,
            })
            .await
            .expect("create user");
        assert_eq!(created.login_name, "operator");
        assert!(!created.is_admin);
        assert!(created.is_active);

        let (total, users) = store.list_users_page(0, 20).await.expect("list users");
        assert_eq!(total, 2);
        assert_eq!(users.len(), 2);
        assert!(
            store
                .get_active_user(created.user_id)
                .await
                .expect("active user")
                .is_some()
        );

        pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
