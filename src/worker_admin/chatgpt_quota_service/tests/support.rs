use std::{future::Future, panic::AssertUnwindSafe, sync::Arc};

use chrono::{DateTime, Utc};
use futures::{FutureExt, future::BoxFuture};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    db::{self, ChatgptQuotaSnapshotCreate, ChatgptQuotaSnapshotSource, ConfigRepository},
    worker_admin::{
        chatgpt_quota_service::{ChatGptQuotaFetcher, ChatGptQuotaService},
        handlers::chatgpt_backend::{ChatgptQuota, ChatgptQuotaWindow},
    },
};

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// Only an absent pair of dedicated keys skips. A configured target that is
/// malformed or disagrees with its confirmed identity fails here rather than
/// degrading into a silent skip.
pub(crate) async fn test_pool() -> Option<PgPool> {
    match test_pg_target::optional_pool(4).await {
        Ok(Some(pool)) => Some(pool),
        Ok(None) => {
            eprintln!(
                "skipped ChatGPT quota service DB setup: neither {} nor {} is configured",
                test_pg_target::DATABASE_URL_KEY,
                test_pg_target::CONFIRMED_TARGET_KEY
            );
            None
        }
        Err(error) => panic!("quota service test target is not usable: {error}"),
    }
}

pub(crate) async fn create_endpoint(pool: &PgPool) -> Uuid {
    let endpoint_id = Uuid::new_v4();
    create_endpoint_with_id(pool, endpoint_id).await;
    endpoint_id
}

/// ID-taking writer split so the owned-fixture context can register the id
/// BEFORE the first write lands; the INSERT shape is unchanged.
pub(crate) async fn create_endpoint_with_id(pool: &PgPool, endpoint_id: Uuid) {
    insert_endpoint_row(pool, endpoint_id).await;
    insert_oauth_row(pool, endpoint_id).await;
}

/// First of the two fixture writes, exposed for the controlled
/// between-write failure case (the second write never lands).
pub(crate) async fn insert_endpoint_row(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
          native_api, native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, 'openai', NULL, 'https://example.invalid/v1', \
                 'responses', 'manual', 'quota-service-fixture-key', true)",
    )
    .bind(endpoint_id)
    .bind(format!("quota-service-{endpoint_id}"))
    .execute(pool)
    .await
    .expect("insert isolated service endpoint");
}

/// Second of the two fixture writes.
pub(crate) async fn insert_oauth_row(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query(
        "INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token) \
         VALUES ($1, 'fixture-access', 'fixture-refresh')",
    )
    .bind(endpoint_id)
    .execute(pool)
    .await
    .expect("insert fake OAuth presence for isolated endpoint");
}

/// Selected-request activity with a caller-supplied id so the context can
/// register it before the write, mirroring the endpoint split above.
pub(crate) async fn insert_request_activity(
    pool: &PgPool,
    endpoint_id: Uuid,
    request_id: Uuid,
    created_at: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO request_records (request_id, endpoint_id, path, created_at) \
         VALUES ($1, $2, '/v1/responses', $3)",
    )
    .bind(request_id)
    .bind(endpoint_id)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("insert isolated selected-request activity");
}

pub(crate) async fn endpoint(pool: &PgPool, endpoint_id: Uuid) -> db::ProviderEndpoint {
    db::get_endpoint(pool, endpoint_id)
        .await
        .expect("load isolated service endpoint")
        .expect("service endpoint exists")
}

pub(crate) async fn cleanup_orphan_fixtures(pool: &PgPool) {
    let endpoint_id = Uuid::parse_str("34dbe486-898c-4965-8790-f1643aec2e7d")
        .expect("known review fixture UUID is valid");
    let expected_name = format!("quota-service-{endpoint_id}");
    let name = sqlx::query_scalar::<_, String>(
        "SELECT name FROM provider_endpoints WHERE endpoint_id = $1",
    )
    .bind(endpoint_id)
    .fetch_optional(pool)
    .await
    .expect("verify exact review fixture before cleanup");
    if name.as_deref() == Some(expected_name.as_str()) {
        cleanup_endpoint(pool, endpoint_id).await;
    } else if name.is_some() {
        panic!("refusing cleanup because exact fixture ownership did not match");
    }
}

pub(crate) async fn cleanup_endpoint(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query("DELETE FROM request_records WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove isolated request activity");
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove isolated service endpoint");
}

/// Fallible owned remover: SQL errors return instead of panicking, so a
/// DELETE failure inside the owned runner aggregates with the body outcome
/// instead of unwinding past pool closure. Legacy direct callers keep
/// [`cleanup_endpoint`] with its existing panic expectations.
pub(crate) async fn try_cleanup_endpoint(pool: &PgPool, endpoint_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM request_records WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .map_err(|error| anyhow::anyhow!("remove isolated request activity: {error}"))?;
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .map_err(|error| anyhow::anyhow!("remove isolated service endpoint: {error}"))?;
    Ok(())
}

/// Owned remover signature: only registered owned ids ever reach it, and the
/// default is the real fallible remover. Controlled remover-error cases
/// override it; the override never widens scope beyond the registered id.
type OwnedDeleter = for<'a> fn(&'a PgPool, Uuid) -> BoxFuture<'a, anyhow::Result<()>>;

fn real_owned_deleter<'a>(
    pool: &'a PgPool,
    endpoint_id: Uuid,
) -> BoxFuture<'a, anyhow::Result<()>> {
    Box::pin(try_cleanup_endpoint(pool, endpoint_id))
}

/// Fixtures one test owns: every id is registered BEFORE its first write, so
/// a failure or panic between writes still finds the id for cleanup. Shared
/// behind locks so the body owns a clone while the runner retains its own
/// handle for guaranteed cleanup. Only registered ids are ever deleted, and
/// only when the row still carries the exact `quota-service-{id}` fixture
/// name — anything else (historical orphans included) is refused rather than
/// touched.
#[derive(Clone)]
pub(crate) struct OwnedFixtures {
    pool: PgPool,
    endpoint_ids: Arc<std::sync::Mutex<Vec<Uuid>>>,
    request_ids: Arc<std::sync::Mutex<Vec<Uuid>>>,
    workers: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    deleter: Arc<std::sync::Mutex<OwnedDeleter>>,
}

impl OwnedFixtures {
    fn lock_ids(ids: &std::sync::Mutex<Vec<Uuid>>) -> std::sync::MutexGuard<'_, Vec<Uuid>> {
        ids.lock().expect("owned fixture ids stay lockable")
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) fn endpoint_ids(&self) -> Vec<Uuid> {
        Self::lock_ids(&self.endpoint_ids).clone()
    }

    pub(crate) fn request_ids(&self) -> Vec<Uuid> {
        Self::lock_ids(&self.request_ids).clone()
    }

    /// Registers the id first, then writes: a partial creation still cleans
    /// up because the id outlives the failed write. The lock is never held
    /// across the write await.
    pub(crate) async fn endpoint(&self) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        Self::lock_ids(&self.endpoint_ids).push(endpoint_id);
        create_endpoint_with_id(&self.pool, endpoint_id).await;
        endpoint_id
    }

    pub(crate) async fn request_activity(
        &self,
        endpoint_id: Uuid,
        created_at: DateTime<Utc>,
    ) -> Uuid {
        let request_id = Uuid::new_v4();
        Self::lock_ids(&self.request_ids).push(request_id);
        insert_request_activity(&self.pool, endpoint_id, request_id, created_at).await;
        request_id
    }

    /// Tracks one background worker so cleanup joins owned async work before
    /// touching the database; never used for foreign or shared handles.
    pub(crate) fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.workers
            .lock()
            .expect("owned workers stay lockable")
            .push(tokio::spawn(future));
    }

    /// Registers a caller-created id before its writes, for controlled
    /// partial-creation cases that write rows directly instead of through
    /// [`OwnedFixtures::endpoint`].
    pub(crate) fn register_endpoint(&self, endpoint_id: Uuid) {
        Self::lock_ids(&self.endpoint_ids).push(endpoint_id);
    }

    /// Overrides the owned remover for controlled remover-error cases only.
    /// The override still receives only registered owned ids, and the exact
    /// name and billing guards still run first.
    pub(crate) fn override_deleter(&self, deleter: OwnedDeleter) {
        *self.deleter.lock().expect("owned deleter stays lockable") = deleter;
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        // Every registered worker is awaited even when one fails: an early
        // return here would detach later workers and skip owned cleanup.
        let workers =
            std::mem::take(&mut *self.workers.lock().expect("owned workers stay lockable"));
        let mut failures: Vec<String> = Vec::new();
        for worker in workers {
            if let Err(error) = worker.await {
                failures.push(format!("owned worker did not finish: {error}"));
            }
        }
        // Owned cleanup is always attempted, then the pool is closed, and all
        // failures are reported together; the runner preserves the body
        // outcome alongside this result.
        if let Err(error) = self.cleanup_owned().await {
            failures.push(format!("{error:#}"));
        }
        self.pool.close().await;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(failures.join("; ")))
        }
    }

    async fn cleanup_owned(&self) -> anyhow::Result<()> {
        for endpoint_id in Self::lock_ids(&self.endpoint_ids).iter() {
            let charges: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM usage_charges AS charge \
                 JOIN request_records AS record ON record.event_id = charge.event_id \
                 WHERE record.endpoint_id = $1",
            )
            .bind(endpoint_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| anyhow::anyhow!("count owned charge references: {error}"))?;
            if charges != 0 {
                anyhow::bail!(
                    "refusing cleanup of {endpoint_id}: {charges} billing rows reference it; \
                     billing records are never deleted"
                );
            }
        }
        // Every registered fixture is attempted even when one fails, so one
        // remover error cannot strand the rest; failures aggregate.
        let deleter = *self.deleter.lock().expect("owned deleter stays lockable");
        let mut failures: Vec<String> = Vec::new();
        for endpoint_id in Self::lock_ids(&self.endpoint_ids).iter() {
            if let Err(error) = cleanup_owned_endpoint(&self.pool, *endpoint_id, deleter).await {
                failures.push(format!("cleanup of {endpoint_id} failed: {error:#}"));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(failures.join("; ")))
        }
    }
}

/// Deletes exactly one registered fixture: an absent row is already clean, an
/// exact fixture name proceeds through the fallible remover, and any other
/// name refuses instead of deleting something foreign.
async fn cleanup_owned_endpoint(
    pool: &PgPool,
    endpoint_id: Uuid,
    deleter: OwnedDeleter,
) -> anyhow::Result<()> {
    let expected_name = format!("quota-service-{endpoint_id}");
    let name = sqlx::query_scalar::<_, String>(
        "SELECT name FROM provider_endpoints WHERE endpoint_id = $1",
    )
    .bind(endpoint_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| anyhow::anyhow!("verify owned fixture {endpoint_id}: {error}"))?;
    match name.as_deref() {
        None => Ok(()),
        Some(name) if name == expected_name => deleter(pool, endpoint_id).await,
        Some(_) => anyhow::bail!(
            "refusing cleanup of {endpoint_id}: exact fixture ownership did not match"
        ),
    }
}

/// Runs one body with an owned-fixture context: builds the dedicated pool (or
/// skips), runs the body, always awaits cleanup (workers joined, then owned
/// rows), then reports the body's outcome. A body panic resumes after
/// cleanup; a body error returns after cleanup, with a cleanup failure
/// appended rather than replacing the original error. Everything the body
/// touches is owned, so no borrow outlives the case.
pub(crate) async fn with_owned_fixtures<F, Fut>(body: F) -> Option<anyhow::Result<()>>
where
    F: FnOnce(OwnedFixtures) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let Some(pool) = test_pool().await else {
        return None;
    };
    let fixtures = OwnedFixtures {
        pool,
        endpoint_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        request_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        workers: Arc::new(std::sync::Mutex::new(Vec::new())),
        deleter: Arc::new(std::sync::Mutex::new(real_owned_deleter)),
    };
    let outcome = AssertUnwindSafe(body(fixtures.clone()))
        .catch_unwind()
        .await;
    let cleanup_outcome = fixtures.cleanup().await;
    Some(match outcome {
        Err(payload) => {
            if let Err(error) = cleanup_outcome {
                eprintln!("QUOTA_FIXTURE_CLEANUP_FAILED: {error:#}");
            }
            std::panic::resume_unwind(payload);
        }
        Ok(Err(error)) => {
            if let Err(cleanup_error) = cleanup_outcome {
                return Some(Err(anyhow::anyhow!(
                    "{error:#}; cleanup also failed: {cleanup_error:#}"
                )));
            }
            Err(error)
        }
        Ok(Ok(())) => cleanup_outcome,
    })
}

pub(crate) fn service(pool: &PgPool, fetcher: Arc<dyn ChatGptQuotaFetcher>) -> ChatGptQuotaService {
    ChatGptQuotaService::with_fetcher(pool.clone(), ConfigRepository::postgres(pool), fetcher)
}

pub(crate) fn quota(used_percent: Option<f64>) -> ChatgptQuota {
    ChatgptQuota {
        plan_type: Some("plus".to_string()),
        limit_reached: Some(false),
        primary: Some(ChatgptQuotaWindow {
            used_percent,
            limit_window_seconds: Some(604_800),
            reset_after_seconds: None,
            reset_at: None,
        }),
        secondary: None,
        has_credits: None,
        unlimited_credits: None,
        credits_balance: None,
    }
}

pub(crate) async fn seed_snapshot(
    pool: &PgPool,
    endpoint_id: Uuid,
    observed_at: chrono::DateTime<Utc>,
    used_percent: Option<f64>,
) {
    let attempted_at = Utc::now();
    let owner = Uuid::new_v4();
    assert!(
        db::quota_snapshots::acquire_refresh_lease(
            pool,
            endpoint_id,
            owner,
            attempted_at,
            attempted_at + chrono::Duration::seconds(45),
        )
        .await
        .expect("acquire seed lease")
    );
    assert!(
        db::quota_snapshots::complete_refresh_success(
            pool,
            owner,
            ChatgptQuotaSnapshotCreate {
                endpoint_id,
                observed_at,
                plan_type: Some("plus".to_string()),
                limit_reached: Some(false),
                windows: json!([{
                    "source_window": "primary",
                    "window_seconds": 604_800,
                    "used_percent": used_percent,
                    "remaining_percent": used_percent.map(|used| 100.0 - used),
                    "availability": if used_percent.is_some() { "known" } else { "unknown" }
                }]),
                source: ChatgptQuotaSnapshotSource::Manual,
            },
        )
        .await
        .expect("persist seed observation")
    );
}
