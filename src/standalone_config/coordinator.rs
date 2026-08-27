use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{Row, SqlitePool};

/// SQLite coordinator state is durable and transactionally safe for a local
/// host using WAL and the configured busy timeout. It must not be treated as
/// safe coordination over an arbitrary network filesystem.
#[derive(Clone, Debug)]
pub struct StandaloneCoordinatorStore {
    pool: SqlitePool,
}

impl StandaloneCoordinatorStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(crate) async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<String>> {
        let now = unix_seconds();
        Ok(standalone_query!("src/sql/standalone/coordinator_get.sql")
            .bind(namespace)
            .bind(key)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.try_get("payload"))
            .transpose()?)
    }

    pub(crate) async fn put(
        &self,
        namespace: &str,
        key: &str,
        payload: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<()> {
        let now = unix_seconds();
        let expires_at = now.saturating_add(ttl_seconds.max(1) as i64);
        standalone_query!("src/sql/standalone/coordinator_upsert.sql")
            .bind(namespace)
            .bind(key)
            .bind(payload)
            .bind(expires_at)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn get_or_insert(
        &self,
        namespace: &str,
        key: &str,
        payload: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<String> {
        let mut tx = self.pool.begin().await?;
        let now = unix_seconds();
        standalone_query!("src/sql/standalone/coordinator_delete_expired.sql")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        standalone_query!("src/sql/standalone/coordinator_insert_if_absent.sql")
            .bind(namespace)
            .bind(key)
            .bind(payload)
            .bind(now.saturating_add(ttl_seconds.max(1) as i64))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        let current = standalone_query!("src/sql/standalone/coordinator_get.sql")
            .bind(namespace)
            .bind(key)
            .bind(now)
            .fetch_one(&mut *tx)
            .await?
            .try_get::<String, _>("payload")?;
        standalone_query!("src/sql/standalone/coordinator_refresh.sql")
            .bind(now.saturating_add(ttl_seconds.max(1) as i64))
            .bind(now)
            .bind(namespace)
            .bind(key)
            .bind(&current)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(current)
    }

    pub(crate) async fn replace_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &str,
        replacement: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<bool> {
        let now = unix_seconds();
        let result = standalone_query!("src/sql/standalone/coordinator_replace.sql")
            .bind(replacement)
            .bind(now.saturating_add(ttl_seconds.max(1) as i64))
            .bind(now)
            .bind(namespace)
            .bind(key)
            .bind(expected)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        let result = standalone_query!("src/sql/standalone/coordinator_delete.sql")
            .bind(namespace)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub(crate) async fn acquire_lease(
        &self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<bool> {
        let now = unix_seconds();
        let row = standalone_query!("src/sql/standalone/coordinator_acquire_lease.sql")
            .bind(key)
            .bind(owner)
            .bind(now.saturating_add(ttl_seconds.max(1) as i64))
            .bind(now)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some_and(|row| {
            row.try_get::<String, _>("owner_id")
                .is_ok_and(|value| value == owner)
        }))
    }

    #[cfg(test)]
    pub(crate) async fn refresh_lease(
        &self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<bool> {
        let now = unix_seconds();
        let result = standalone_query!("src/sql/standalone/coordinator_refresh_lease.sql")
            .bind(now.saturating_add(ttl_seconds.max(1) as i64))
            .bind(now)
            .bind(key)
            .bind(owner)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub(crate) async fn release_lease(&self, key: &str, owner: &str) -> anyhow::Result<()> {
        standalone_query!("src/sql/standalone/coordinator_release_lease.sql")
            .bind(key)
            .bind(owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
