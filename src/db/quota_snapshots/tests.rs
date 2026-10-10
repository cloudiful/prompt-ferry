use super::{
    complete_refresh_success, latest_snapshot, list_snapshot_history, quota_history_endpoint,
    validate_windows,
};
use crate::db::types::{ChatgptQuotaSnapshotCreate, ChatgptQuotaSnapshotSource};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use test_support::{cleanup_endpoint, create_endpoint, known_window, test_pool};

#[tokio::test]
async fn snapshot_history_is_endpoint_scoped_bounded_and_includes_disabled_endpoints() {
    let Some(pool) = test_support::test_pool().await else {
        return;
    };
    let now = Utc::now();
    let endpoint_id = test_support::create_endpoint(&pool, "openai", false, true).await;
    let other_endpoint = test_support::create_endpoint(&pool, "openai", true, true).await;
    let expired_id = test_support::record_snapshot(
        &pool,
        endpoint_id,
        now - chrono::Duration::days(31),
        ChatgptQuotaSnapshotSource::Manual,
    )
    .await;
    let oldest_recent_id = test_support::record_snapshot(
        &pool,
        endpoint_id,
        now - chrono::Duration::days(29),
        ChatgptQuotaSnapshotSource::Request,
    )
    .await;
    let middle_id = test_support::record_snapshot(
        &pool,
        endpoint_id,
        now - chrono::Duration::days(10),
        ChatgptQuotaSnapshotSource::Periodic,
    )
    .await;
    let newest_id =
        test_support::record_snapshot(&pool, endpoint_id, now, ChatgptQuotaSnapshotSource::Manual)
            .await;
    test_support::record_snapshot(
        &pool,
        other_endpoint,
        now,
        ChatgptQuotaSnapshotSource::Request,
    )
    .await;

    let page = list_snapshot_history(&pool, endpoint_id, None, 2)
        .await
        .unwrap();
    assert_eq!(
        page.iter()
            .map(|snapshot| snapshot.snapshot_id)
            .collect::<Vec<_>>(),
        vec![newest_id, middle_id]
    );
    let next = list_snapshot_history(&pool, endpoint_id, Some(middle_id), 2)
        .await
        .unwrap();
    assert_eq!(
        next.iter()
            .map(|snapshot| snapshot.snapshot_id)
            .collect::<Vec<_>>(),
        vec![oldest_recent_id]
    );
    assert!(
        list_snapshot_history(&pool, endpoint_id, Some(oldest_recent_id), 2)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        list_snapshot_history(&pool, other_endpoint, None, 2)
            .await
            .unwrap()
            .iter()
            .all(|snapshot| snapshot.endpoint_id == other_endpoint)
    );
    assert_eq!(
        quota_history_endpoint(&pool, endpoint_id)
            .await
            .unwrap()
            .unwrap()
            .provider,
        "openai"
    );
    assert!(
        quota_history_endpoint(&pool, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !list_snapshot_history(&pool, endpoint_id, None, 201)
            .await
            .unwrap()
            .iter()
            .any(|snapshot| snapshot.snapshot_id == expired_id)
    );
    test_support::cleanup_endpoint(&pool, endpoint_id).await;
    test_support::cleanup_endpoint(&pool, other_endpoint).await;
}

#[test]
fn normalized_windows_reject_raw_or_inconsistent_payload_fields() {
    validate_windows(&test_support::known_window("primary", 0.0))
        .expect("known zero remains valid");
    validate_windows(&json!([{
        "source_window": "secondary",
        "window_seconds": 604_800,
        "availability": "unknown"
    }]))
    .expect("unknown windows retain known duration");
    assert!(
        validate_windows(
            &json!([{"source_window":"primary","availability":"unknown","account_id":"raw"}])
        )
        .is_err()
    );
    assert!(validate_windows(&json!([{"source_window":"primary","availability":"known","used_percent":120,"remaining_percent":0}])).is_err());
}

#[tokio::test]
async fn success_without_an_owned_lease_rolls_back_the_snapshot_insert() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true, true).await;
    assert!(
        !complete_refresh_success(
            &pool,
            Uuid::new_v4(),
            ChatgptQuotaSnapshotCreate {
                endpoint_id,
                observed_at: Utc::now(),
                plan_type: Some("plus".to_string()),
                limit_reached: Some(false),
                windows: known_window("primary", 75.0),
                source: ChatgptQuotaSnapshotSource::Manual,
            },
        )
        .await
        .unwrap()
    );
    assert!(latest_snapshot(&pool, endpoint_id).await.unwrap().is_none());
    cleanup_endpoint(&pool, endpoint_id).await;
}

pub(super) mod test_support {
    use chrono::{DateTime, Duration, Utc};
    use serde_json::{Value, json};
    use sqlx::{
        PgPool,
        postgres::{PgConnectOptions, PgPoolOptions},
    };
    use std::str::FromStr;
    use uuid::Uuid;

    use crate::db::{
        ChatgptQuotaSnapshotCreate, ChatgptQuotaSnapshotSource,
        quota_snapshots::{acquire_refresh_lease, complete_refresh_success, latest_snapshot},
    };

    pub(in crate::db::quota_snapshots) async fn test_pool() -> Option<PgPool> {
        let url = match std::env::var("PROMPT_FERRY_QUOTA_TEST_DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!(
                    "skipped quota snapshot database setup: PROMPT_FERRY_QUOTA_TEST_DATABASE_URL is unset"
                );
                return None;
            }
        };
        let parsed = PgConnectOptions::from_str(&url)
            .unwrap_or_else(|_| panic!("quota snapshot test database URL is invalid"));
        assert_eq!(parsed.get_host(), "127.0.0.1");
        assert_eq!(parsed.get_database(), Some("quota_759_test"));
        let options = PgConnectOptions::new_without_pgpass()
            .host("127.0.0.1")
            .port(parsed.get_port())
            .username(parsed.get_username())
            .database("quota_759_test");
        Some(
            PgPoolOptions::new()
                .max_connections(4)
                .connect_with(options)
                .await
                .unwrap_or_else(|_| panic!("quota snapshot test database connection failed")),
        )
    }

    pub(in crate::db::quota_snapshots) async fn create_endpoint(
        pool: &PgPool,
        provider: &str,
        enabled: bool,
        with_oauth: bool,
    ) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO provider_endpoints \
             (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
              native_api, native_api_source, api_key, enabled) \
             VALUES ($1, 'admin', NULL, $2, $3, NULL, 'https://example.invalid/v1', \
                     'responses', 'manual', 'quota-fixture-key', $4)",
        )
        .bind(endpoint_id)
        .bind(format!("quota-test-{endpoint_id}"))
        .bind(provider)
        .bind(enabled)
        .execute(pool)
        .await
        .expect("insert isolated quota test endpoint");
        if with_oauth {
            sqlx::query(
                "INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token) \
                 VALUES ($1, 'fixture-access', 'fixture-refresh')",
            )
            .bind(endpoint_id)
            .execute(pool)
            .await
            .expect("insert fake OAuth presence for isolated endpoint");
        }
        endpoint_id
    }

    pub(in crate::db::quota_snapshots) async fn cleanup_endpoint(pool: &PgPool, endpoint_id: Uuid) {
        sqlx::query("DELETE FROM request_records WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .execute(pool)
            .await
            .expect("remove isolated request activity fixture");
        sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .execute(pool)
            .await
            .expect("remove isolated quota endpoint fixture");
    }

    pub(in crate::db::quota_snapshots) fn known_window(source: &str, remaining: f64) -> Value {
        json!([{
            "source_window": source,
            "window_seconds": 18_000,
            "used_percent": 100.0 - remaining,
            "remaining_percent": remaining,
            "availability": "known"
        }])
    }

    pub(in crate::db::quota_snapshots) async fn record_snapshot(
        pool: &PgPool,
        endpoint_id: Uuid,
        observed_at: DateTime<Utc>,
        source: ChatgptQuotaSnapshotSource,
    ) -> i64 {
        let attempted_at = Utc::now();
        let owner = Uuid::new_v4();
        assert!(
            acquire_refresh_lease(
                pool,
                endpoint_id,
                owner,
                attempted_at,
                attempted_at + Duration::seconds(45),
            )
            .await
            .expect("acquire endpoint refresh lease")
        );
        assert!(
            complete_refresh_success(
                pool,
                owner,
                ChatgptQuotaSnapshotCreate {
                    endpoint_id,
                    observed_at,
                    plan_type: Some("plus".to_string()),
                    limit_reached: Some(false),
                    windows: known_window("primary", 75.0),
                    source,
                },
            )
            .await
            .expect("commit successful quota snapshot")
        );
        latest_snapshot(pool, endpoint_id)
            .await
            .expect("load latest snapshot")
            .expect("snapshot was inserted")
            .snapshot_id
    }
}
