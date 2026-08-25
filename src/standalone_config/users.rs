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
    use std::path::PathBuf;

    use anyhow::anyhow;

    use super::SqliteUserStore;

    fn database_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "prompt-ferry-users-{}.sqlite",
            uuid::Uuid::new_v4()
        ))
    }

    #[tokio::test]
    async fn sqlite_bootstrap_generates_or_uses_configured_password_and_persists_users() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        // An empty configured password generates one on a fresh database.
        let generated = store
            .bootstrap_admin("admin", "")
            .await
            .expect("generated bootstrap admin")
            .expect("generated password");
        assert!(generated.len() >= 24);
        assert_ne!(generated, "admin");
        let stored = store
            .get_user_password_by_login("admin")
            .await
            .expect("load password")
            .expect("admin user");
        assert!(crate::keys::verify_password(
            &generated,
            &stored.password_hash
        ));

        // An existing admin is idempotent and never regenerated.
        assert!(
            store
                .bootstrap_admin("admin", "")
                .await
                .expect("existing admin is idempotent")
                .is_none()
        );
        let unchanged = store
            .get_user_password_by_login("admin")
            .await
            .expect("load password")
            .expect("admin user");
        assert_eq!(unchanged.password_hash, stored.password_hash);

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

    #[tokio::test]
    async fn sqlite_bootstrap_prefers_configured_password_on_fresh_database() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        assert!(
            store
                .bootstrap_admin("admin", "correct horse battery staple")
                .await
                .expect("configured bootstrap admin")
                .is_none()
        );
        let stored = store
            .get_user_password_by_login("admin")
            .await
            .expect("load password")
            .expect("admin user");
        assert!(stored.is_admin);
        assert!(stored.is_active);
        assert!(crate::keys::verify_password(
            "correct horse battery staple",
            &stored.password_hash
        ));

        pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sqlite_bootstrap_persist_failure_leaves_no_admin_and_retry_recovers() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        // A failing protected-file write must abort before the admin row is
        // committed so the next start can regenerate a retrievable password.
        let error = store
            .ensure_bootstrap_admin("admin", None, |_| {
                Err(anyhow!("simulated bootstrap-admin.txt write failure"))
            })
            .await
            .expect_err("persist failure propagates");
        assert!(error.to_string().contains("write failure"));
        assert!(
            store
                .get_user_password_by_login("admin")
                .await
                .expect("load admin")
                .is_none(),
            "no active admin may exist after a failed password publication"
        );

        // Retry on the next start succeeds and stores exactly the published
        // password.
        let mut published: Option<String> = None;
        store
            .ensure_bootstrap_admin("admin", None, |generated| {
                published = Some(generated.to_string());
                Ok(generated.to_string())
            })
            .await
            .expect("retry creates admin");
        let generated = published.expect("generated password passed to persistence");
        let stored = store
            .get_user_password_by_login("admin")
            .await
            .expect("load admin")
            .expect("admin user");
        assert!(crate::keys::verify_password(
            &generated,
            &stored.password_hash
        ));

        pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sqlite_bootstrap_commits_the_effective_password_returned_by_the_resolver() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        // Simulate a racing starter that already published a different
        // password: the resolver reuses it and the database must contain the
        // effective (winner) password, never the losing candidate.
        const WINNER: &str = "winner-password-published-by-another-process";
        let mut candidate_seen: Option<String> = None;
        store
            .ensure_bootstrap_admin("admin", None, |candidate| {
                candidate_seen = Some(candidate.to_string());
                Ok(WINNER.to_string())
            })
            .await
            .expect("resolver-driven bootstrap admin");

        let candidate = candidate_seen.expect("candidate passed to resolver");
        assert_ne!(candidate, WINNER);
        let stored = store
            .get_user_password_by_login("admin")
            .await
            .expect("load admin")
            .expect("admin user");
        assert!(crate::keys::verify_password(WINNER, &stored.password_hash));
        assert!(!crate::keys::verify_password(
            &candidate,
            &stored.password_hash
        ));

        pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sqlite_bootstrap_rejects_empty_login_even_when_generating_password() {
        let path = database_path();
        let pool = crate::db::connect_sqlite(&path).await.expect("SQLite pool");
        crate::db::migrate_standalone(&pool)
            .await
            .expect("SQLite migrations");
        let store = SqliteUserStore::new(pool.clone());

        for login in ["", "   "] {
            let error = store
                .ensure_bootstrap_admin(login, None, |candidate| Ok(candidate.to_string()))
                .await
                .expect_err("empty login is rejected");
            assert!(error.to_string().contains("login must not be empty"));
        }
        assert!(
            store.list_users().await.expect("list users").is_empty(),
            "a rejected empty login must not create any account"
        );

        pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
