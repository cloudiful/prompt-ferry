use super::*;
use crate::{
    db,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse},
};

#[test]
fn local_estimator_is_conservative_for_small_payloads() {
    assert_eq!(estimate_input_tokens(br#"{"model":"MiniMax-M3"}"#), 6);
    assert!(estimate_input_tokens("中文请求".as_bytes()) >= 1);
}

pub(super) fn opencode_go_usage(key_id: Uuid, remaining: f64) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys: vec![TokenPlanKeyUsage {
            key_id,
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            quota_status: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: Some(OpencodeGoWindowUsage {
                status: None,
                percent: Some(100.0 - remaining),
                resets_at: None,
            }),
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
            quota_windows: None,
        }],
    }
}

#[tokio::test]
async fn reservations_damp_percent_windows_but_never_starve_them() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let other_key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    assert_eq!(
        cache.key_weight_percent_now(endpoint_id, key_id, None),
        Some(80.0)
    );
    for _ in 0..5 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 1_000);
    }
    let damped = cache
        .key_weight_percent_now(endpoint_id, key_id, None)
        .expect("weighted key");
    assert!(damped < 80.0, "reservations must damp the weight: {damped}");
    assert!(damped > 0.0);
    for _ in 0..1_000_000 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 1);
    }
    let floor = cache
        .key_weight_percent_now(endpoint_id, key_id, None)
        .expect("weighted key");
    assert!(floor.is_finite() && floor > 0.0, "floor={floor}");
    assert_eq!(
        cache.key_weight_percent_now(endpoint_id, other_key_id, None),
        None
    );
}

#[tokio::test]
async fn raw_remaining_ignores_urgency_and_reservations() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 30.0))
        .await;
    for _ in 0..50 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 10_000);
    }
    assert_eq!(
        cache.key_remaining_percent_now(endpoint_id, key_id, None),
        Some(30.0),
        "exhaustion checks must keep the raw window percent"
    );
}

#[tokio::test]
async fn invalidate_drops_freshness_so_next_refresh_refetches() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    assert_eq!(
        cache
            .fresh_snapshot(endpoint_id)
            .await
            .expect("stored snapshot is fresh")
            .keys[0]
            .key_id,
        key_id
    );
    cache.invalidate(endpoint_id).await;
    assert!(cache.fresh_snapshot(endpoint_id).await.is_none());
    assert!(
        cache
            .key_weight_percent_now(endpoint_id, key_id, None)
            .is_none()
    );
    assert!(cache.raw_snapshot(endpoint_id).await.is_some());
}

#[tokio::test]
async fn fresh_snapshot_serves_the_stored_payload_for_its_own_endpoint_only() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let other_endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    let served = cache
        .fresh_snapshot(endpoint_id)
        .await
        .expect("fresh entry");
    assert_eq!(served.keys.len(), 1);
    assert_eq!(served.keys[0].key_id, key_id);
    assert_eq!(
        served.keys[0]
            .opencodego_rolling
            .as_ref()
            .and_then(|window| window.percent),
        Some(20.0)
    );
    assert!(cache.fresh_snapshot(other_endpoint_id).await.is_none());
}

#[cfg(test)]
mod persist_tests {
    use super::opencode_go_usage;
    use super::*;
    use crate::db::QuotaObservationSource;
    use crate::worker_admin_types::{
        CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
    };

    #[allow(dead_code)]
    mod test_pg_target {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/test_pg_target.rs"
        ));
    }

    async fn test_pool() -> Option<PgPool> {
        match test_pg_target::optional_pool(4).await {
            Ok(pool) => pool,
            Err(error) => {
                panic!("quota observation persist test target is not usable: {error}")
            }
        }
    }

    async fn create_endpoint(pool: &PgPool) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO provider_endpoints \
             (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
              native_api, native_api_source, api_key, enabled) \
             VALUES ($1, 'admin', NULL, $2, 'opencode_go', NULL, 'https://example.invalid/v1', \
                     'responses', 'manual', 'persist-fixture-key', true)",
        )
        .bind(endpoint_id)
        .bind(format!("quota-persist-{endpoint_id}"))
        .execute(pool)
        .await
        .expect("insert isolated persist endpoint");
        endpoint_id
    }

    async fn create_key(pool: &PgPool, endpoint_id: Uuid, key_label: &str) -> Uuid {
        let key_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, enabled) \
             VALUES ($1, $2, $3, 'persist-fixture-secret', \
                     (SELECT COUNT(*) FROM endpoint_api_keys WHERE endpoint_id = $2), true)",
        )
        .bind(key_id)
        .bind(endpoint_id)
        .bind(key_label)
        .execute(pool)
        .await
        .expect("insert isolated persist key");
        key_id
    }

    async fn cleanup(pool: &PgPool, endpoint_id: Uuid) {
        sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .execute(pool)
            .await
            .expect("remove isolated persist fixtures");
    }

    async fn row_count(pool: &PgPool, endpoint_id: Uuid, key_id: Uuid) -> usize {
        db::quota_key_observations::list_observations(pool, endpoint_id, key_id, None, 200)
            .await
            .expect("list isolated observations")
            .len()
    }

    fn invalid_window() -> CanonicalQuotaWindow {
        CanonicalQuotaWindow {
            source_id: "opencode_go:rolling".to_string(),
            kind: CanonicalQuotaWindowKind::Rolling,
            source_model: None,
            unit: None,
            duration_seconds: None,
            is_default_scope: true,
            availability: SubscriptionWindowAvailability::Known,
            remaining_percent: Some(f64::NAN),
            current: None,
            limit: None,
            remaining: None,
            reset_at: None,
        }
    }

    #[tokio::test]
    async fn new_results_persist_once_and_cache_hits_write_nothing() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = create_endpoint(&pool).await;
        let key_id = create_key(&pool, endpoint_id, "live-key").await;
        let usage = opencode_go_usage(key_id, 80.0);

        cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                true,
                QuotaObservationSource::Request,
                || async { Ok(Some(usage.clone())) },
            )
            .await
            .expect("forced fetch persists")
            .expect("fetch returns usage");
        assert_eq!(row_count(&pool, endpoint_id, key_id).await, 1);
        let latest = db::quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
            .await
            .unwrap()
            .expect("persisted row");
        assert!(latest.ok);
        assert_eq!(latest.source, QuotaObservationSource::Request);
        assert_eq!(latest.windows.len(), 1);
        assert_eq!(latest.windows[0].remaining_percent, Some(80.0));

        // A fresh cache hit never reaches the fetch closure, so no row follows.
        cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                false,
                QuotaObservationSource::Request,
                || async { panic!("cache hit must not fetch") },
            )
            .await
            .expect("cache hit serves")
            .expect("usage served");
        assert_eq!(row_count(&pool, endpoint_id, key_id).await, 1);
        cleanup(&pool, endpoint_id).await;
    }

    #[tokio::test]
    async fn partial_key_failures_persist_honestly() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = create_endpoint(&pool).await;
        let good_key = create_key(&pool, endpoint_id, "good-key").await;
        let bad_key = create_key(&pool, endpoint_id, "bad-key").await;
        let mut usage = opencode_go_usage(good_key, 80.0);
        let mut bad = opencode_go_usage(bad_key, 50.0)
            .keys
            .pop()
            .expect("one key");
        bad.ok = false;
        bad.opencodego_rolling = None;
        bad.quota_windows = None;
        bad.status = Some(401);
        bad.error_code = Some("auth".to_string());
        usage.keys.push(bad);

        cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                true,
                QuotaObservationSource::Manual,
                || async { Ok(Some(usage.clone())) },
            )
            .await
            .expect("partial fetch persists")
            .expect("usage served");
        let good = db::quota_key_observations::latest_observation(&pool, endpoint_id, good_key)
            .await
            .unwrap()
            .expect("good row");
        assert!(good.ok);
        assert_eq!(good.windows.len(), 1);
        let bad = db::quota_key_observations::latest_observation(&pool, endpoint_id, bad_key)
            .await
            .unwrap()
            .expect("failed row");
        assert!(!bad.ok);
        assert!(bad.windows.is_empty());
        assert_eq!(bad.status.as_deref(), Some("401"));
        assert_eq!(bad.error_code.as_deref(), Some("auth"));
        cleanup(&pool, endpoint_id).await;
    }

    #[tokio::test]
    async fn invalid_windows_never_block_fresh_usage() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = create_endpoint(&pool).await;
        let key_id = create_key(&pool, endpoint_id, "live-key").await;
        let mut usage = opencode_go_usage(key_id, 80.0);
        usage.keys[0].quota_windows = Some(vec![invalid_window()]);

        let served = cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                true,
                QuotaObservationSource::Request,
                || async { Ok(Some(usage.clone())) },
            )
            .await
            .expect("persist failure keeps the result")
            .expect("usage served");
        assert_eq!(served.keys[0].key_id, key_id);
        assert!(cache.fresh_snapshot(endpoint_id).await.is_some());
        assert_eq!(row_count(&pool, endpoint_id, key_id).await, 0);
        cleanup(&pool, endpoint_id).await;
    }

    #[tokio::test]
    async fn unsupported_fetches_write_no_row() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();

        let served = cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                true,
                QuotaObservationSource::Request,
                || async { Ok(None) },
            )
            .await
            .expect("unsupported fetch resolves");
        assert!(served.is_none());
        assert_eq!(row_count(&pool, endpoint_id, key_id).await, 0);
    }

    #[tokio::test]
    async fn endpoint_failures_record_sanitized_rows_without_secrets() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = create_endpoint(&pool).await;
        let key_id = create_key(&pool, endpoint_id, "live-key").await;

        let result: anyhow::Result<Option<TokenPlanUsageResponse>> = cache
            .refresh_with_source(
                &pool,
                endpoint_id,
                true,
                QuotaObservationSource::Periodic,
                || async { anyhow::bail!("boom carrying secret-key-xyz") },
            )
            .await;
        assert!(result.is_err());
        let row = db::quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
            .await
            .unwrap()
            .expect("failure row");
        assert!(!row.ok);
        assert!(row.windows.is_empty());
        assert!(row.status.is_none());
        assert_eq!(row.error_code.as_deref(), Some("upstream"));
        assert_eq!(row.source, QuotaObservationSource::Periodic);
        assert!(!format!("{row:?}").contains("secret-key-xyz"));
        cleanup(&pool, endpoint_id).await;
    }
}
