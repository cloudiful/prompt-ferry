use super::{
    ObservationQueryError, parse_observation_query, quota_key_observation_history,
    quota_key_observation_trend,
};
use crate::{
    db,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        CanonicalQuotaWindow, CanonicalQuotaWindowKind, QuotaKeyObservationHistoryQuery,
        QuotaKeyObservationTrendQuery, RequestContentLoggingMode, RequestContentLoggingResponse,
        SessionUser, SubscriptionWindowAvailability, UsageRetentionSettings,
    },
};
use axum::http::header;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration as StdDuration;
use uuid::Uuid;

fn test_state_with_pool(pool: PgPool) -> AdminState {
    AdminState::new(AdminStateInit {
        pool: pool.clone(),
        lease_pool: pool.clone(),
        replay_cache: ReplayCache::for_tests(),
        configured_relays: Vec::new(),
        managed_mode: false,
        relay_secret_manager: None,
        redaction_enabled: false,
        model_route_whitelist_enabled: true,
        request_content_logging: RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        },
        usage_retention: UsageRetentionSettings::default(),
        raw_payload_store: None,
        stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
        llm_review_settings: LlmReviewSettings::default(),
        mcp_catalog_cache: McpCatalogCache::new(),
        mcp_catalog_service: McpCatalogService::new(pool.clone(), McpCatalogCache::new()),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
            StdDuration::from_secs(60),
        ),
    })
}

fn lazy_test_state() -> AdminState {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://quota_observation_test@127.0.0.1:1/quota_observation_test")
        .expect("lazy test pool");
    test_state_with_pool(pool)
}

async fn attach_session(state: &AdminState, id: &str, is_admin: bool) {
    state
        .replay_cache
        .write_session(
            id,
            &SessionUser {
                user_id: 1,
                login_name: "quota-observation-test".to_string(),
                display_name: "Quota observation test".to_string(),
                is_admin,
            },
        )
        .await
        .expect("test session");
}

fn admin_headers(session_id: &str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("prompt_ferry_session={session_id}")
            .parse()
            .expect("session cookie"),
    );
    headers
}

async fn get_history(
    state: &AdminState,
    headers: axum::http::HeaderMap,
    endpoint_id: Uuid,
    query: QuotaKeyObservationHistoryQuery,
) -> axum::response::Response {
    quota_key_observation_history(
        axum::extract::State(state.clone()),
        headers,
        axum::extract::Path(endpoint_id),
        axum::extract::Query(query),
    )
    .await
}

async fn get_trend(
    state: &AdminState,
    headers: axum::http::HeaderMap,
    endpoint_id: Uuid,
    query: QuotaKeyObservationTrendQuery,
) -> axum::response::Response {
    quota_key_observation_trend(
        axum::extract::State(state.clone()),
        headers,
        axum::extract::Path(endpoint_id),
        axum::extract::Query(query),
    )
    .await
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("history body reads");
    serde_json::from_slice(&bytes).expect("history body is JSON")
}

#[test]
fn observation_query_defaults_and_enforces_bounded_pages() {
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery::default()).unwrap(),
        (50, None, None)
    );
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: None,
            limit: Some(1),
            before_id: None,
        })
        .unwrap(),
        (1, None, None)
    );
    for limit in [0, 201] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: None,
                limit: Some(limit),
                before_id: None,
            }),
            Err(ObservationQueryError::InvalidLimit)
        );
    }
}

#[test]
fn observation_cursor_and_key_id_reject_bad_shapes() {
    let key = Uuid::new_v4();
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: Some(key.to_string()),
            limit: None,
            before_id: Some(i64::MAX.to_string()),
        })
        .unwrap(),
        (50, Some(i64::MAX), Some(key))
    );
    for before_id in ["0", "-1", "1.5", " 1", "9223372036854775808"] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: None,
                limit: None,
                before_id: Some(before_id.to_string()),
            }),
            Err(ObservationQueryError::InvalidCursor),
            "cursor={before_id}"
        );
    }
    for key_id in ["", "zzz", "8c6e4e55-967c-4267-a45a-24647d2afad"] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: Some(key_id.to_string()),
                limit: None,
                before_id: None,
            }),
            Err(ObservationQueryError::InvalidKey),
            "key_id={key_id}"
        );
    }
}

#[tokio::test]
async fn observation_history_requires_an_admin_session_before_storage() {
    let endpoint_id = Uuid::new_v4();
    let unauthenticated = get_history(
        &lazy_test_state(),
        axum::http::HeaderMap::new(),
        endpoint_id,
        QuotaKeyObservationHistoryQuery::default(),
    )
    .await;
    assert_eq!(
        unauthenticated.status(),
        axum::http::StatusCode::UNAUTHORIZED
    );

    let state = lazy_test_state();
    attach_session(&state, "quota-observation-non-admin", false).await;
    let forbidden = get_history(
        &state,
        admin_headers("quota-observation-non-admin"),
        endpoint_id,
        QuotaKeyObservationHistoryQuery::default(),
    )
    .await;
    assert_eq!(forbidden.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn invalid_observation_parameters_are_rejected_before_storage() {
    let state = lazy_test_state();
    attach_session(&state, "quota-observation-admin", true).await;
    let headers = admin_headers("quota-observation-admin");
    for query in [
        QuotaKeyObservationHistoryQuery {
            key_id: None,
            limit: Some(201),
            before_id: None,
        },
        QuotaKeyObservationHistoryQuery {
            key_id: None,
            limit: None,
            before_id: Some("0".to_string()),
        },
        QuotaKeyObservationHistoryQuery {
            key_id: Some("not-a-uuid".to_string()),
            limit: None,
            before_id: None,
        },
    ] {
        let response = get_history(&state, headers.clone(), Uuid::new_v4(), query).await;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
}

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
        Err(error) => panic!("quota observation history test target is not usable: {error}"),
    }
}

async fn create_endpoint(pool: &PgPool, provider: &str) -> Uuid {
    let endpoint_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO provider_endpoints \
         (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
          native_api, native_api_source, api_key, enabled) \
         VALUES ($1, 'admin', NULL, $2, $3, NULL, 'https://example.invalid/v1', \
                 'responses', 'manual', 'handler-fixture-key', true)",
    )
    .bind(endpoint_id)
    .bind(format!("quota-history-{endpoint_id}"))
    .bind(provider)
    .execute(pool)
    .await
    .expect("insert isolated history endpoint");
    endpoint_id
}

async fn create_key(pool: &PgPool, endpoint_id: Uuid, key_label: &str) -> Uuid {
    let key_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, enabled) \
         VALUES ($1, $2, $3, 'handler-fixture-secret', \
                 (SELECT COUNT(*) FROM endpoint_api_keys WHERE endpoint_id = $2), true)",
    )
    .bind(key_id)
    .bind(endpoint_id)
    .bind(key_label)
    .execute(pool)
    .await
    .expect("insert isolated history key");
    key_id
}

fn known_window() -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: "opencode_go:rolling".to_string(),
        kind: CanonicalQuotaWindowKind::Rolling,
        source_model: None,
        unit: None,
        duration_seconds: None,
        is_default_scope: true,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: Some(80.0),
        current: None,
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

async fn record_observation(
    pool: &PgPool,
    endpoint_id: Uuid,
    key_id: Uuid,
    observed_at: chrono::DateTime<Utc>,
    ok: bool,
) {
    let create = db::QuotaObservationCreate::new(
        endpoint_id,
        key_id,
        db::QuotaObservationProvider::OpencodeGo,
        "handler-key".to_string(),
        db::QuotaObservationSource::Request,
        observed_at,
        ok,
        ok.then(|| "200".to_string()),
        if ok { None } else { Some("auth".to_string()) },
        None,
        None,
        if ok { vec![known_window()] } else { vec![] },
    )
    .expect("valid test observation builds");
    db::quota_key_observations::insert_observation(pool, &create)
        .await
        .expect("insert isolated observation");
}

async fn cleanup(pool: &PgPool, endpoint_id: Uuid) {
    sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await
        .expect("remove isolated history fixtures");
}

#[tokio::test]
async fn history_lists_both_keys_newest_first_with_plan_metadata_absent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-observation-reader", true).await;
    let headers = admin_headers("quota-observation-reader");
    let endpoint_id = create_endpoint(&pool, "opencode_go").await;
    let first_key = create_key(&pool, endpoint_id, "first-key").await;
    let second_key = create_key(&pool, endpoint_id, "second-key").await;
    let base = Utc::now() - Duration::minutes(5);
    record_observation(&pool, endpoint_id, first_key, base, true).await;
    record_observation(
        &pool,
        endpoint_id,
        second_key,
        base + Duration::seconds(1),
        true,
    )
    .await;
    record_observation(
        &pool,
        endpoint_id,
        first_key,
        base + Duration::seconds(2),
        false,
    )
    .await;

    let body = body_json(
        get_history(
            &state,
            headers,
            endpoint_id,
            QuotaKeyObservationHistoryQuery::default(),
        )
        .await,
    )
    .await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["key_id"], first_key.to_string());
    assert_eq!(items[0]["ok"], false);
    assert_eq!(items[0]["error_code"], "auth");
    assert_eq!(items[0]["windows"].as_array().unwrap().len(), 0);
    assert_eq!(items[1]["key_id"], second_key.to_string());
    assert_eq!(items[1]["windows"][0]["remaining_percent"], 80.0);
    assert_eq!(items[1]["windows"][0]["source_id"], "opencode_go:rolling");
    assert!(items[1]["plan_type"].is_null());
    assert_eq!(body["retention_days"], 30);
    assert!(body["next_cursor"].is_null());
    cleanup(&pool, endpoint_id).await;
}

#[tokio::test]
async fn history_key_filter_and_cursor_pagination_hold_endpoint_boundaries() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-observation-pager", true).await;
    let headers = admin_headers("quota-observation-pager");
    let endpoint_id = create_endpoint(&pool, "opencode_go").await;
    let other_endpoint = create_endpoint(&pool, "opencode_go").await;
    let first_key = create_key(&pool, endpoint_id, "first-key").await;
    let second_key = create_key(&pool, endpoint_id, "second-key").await;
    let foreign_key = create_key(&pool, other_endpoint, "foreign-key").await;
    let base = Utc::now() - Duration::minutes(5);
    record_observation(&pool, endpoint_id, first_key, base, true).await;
    record_observation(
        &pool,
        endpoint_id,
        second_key,
        base + Duration::seconds(1),
        true,
    )
    .await;
    record_observation(
        &pool,
        endpoint_id,
        first_key,
        base + Duration::seconds(2),
        true,
    )
    .await;
    record_observation(&pool, other_endpoint, foreign_key, base, true).await;

    let filtered = body_json(
        get_history(
            &state,
            headers.clone(),
            endpoint_id,
            QuotaKeyObservationHistoryQuery {
                key_id: Some(second_key.to_string()),
                limit: None,
                before_id: None,
            },
        )
        .await,
    )
    .await;
    let items = filtered["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["key_id"], second_key.to_string());

    let page = body_json(
        get_history(
            &state,
            headers.clone(),
            endpoint_id,
            QuotaKeyObservationHistoryQuery {
                key_id: None,
                limit: Some(2),
                before_id: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    let cursor = page["next_cursor"].as_str().expect("cursor").to_string();
    let rest = body_json(
        get_history(
            &state,
            headers.clone(),
            endpoint_id,
            QuotaKeyObservationHistoryQuery {
                key_id: None,
                limit: Some(2),
                before_id: Some(cursor),
            },
        )
        .await,
    )
    .await;
    assert_eq!(rest["items"].as_array().unwrap().len(), 1);
    assert!(rest["next_cursor"].is_null());

    // A key from another endpoint stays outside this endpoint's boundary.
    let foreign = body_json(
        get_history(
            &state,
            headers,
            endpoint_id,
            QuotaKeyObservationHistoryQuery {
                key_id: Some(foreign_key.to_string()),
                limit: None,
                before_id: None,
            },
        )
        .await,
    )
    .await;
    assert_eq!(foreign["items"].as_array().unwrap().len(), 0);
    cleanup(&pool, endpoint_id).await;
    cleanup(&pool, other_endpoint).await;
}

#[tokio::test]
async fn history_rejects_missing_and_plain_provider_endpoints() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-observation-guard", true).await;
    let headers = admin_headers("quota-observation-guard");
    let generic_id = create_endpoint(&pool, "generic").await;

    let missing = get_history(
        &state,
        headers.clone(),
        Uuid::new_v4(),
        QuotaKeyObservationHistoryQuery::default(),
    )
    .await;
    assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
    let plain = get_history(
        &state,
        headers,
        generic_id,
        QuotaKeyObservationHistoryQuery::default(),
    )
    .await;
    assert_eq!(plain.status(), axum::http::StatusCode::NOT_FOUND);
    cleanup(&pool, generic_id).await;
}

#[tokio::test]
async fn removed_key_rows_stay_queryable_without_current_config_membership() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-observation-removed-key", true).await;
    let headers = admin_headers("quota-observation-removed-key");
    let endpoint_id = create_endpoint(&pool, "opencode_go").await;
    let live_key = create_key(&pool, endpoint_id, "live-key").await;
    let removed_key = create_key(&pool, endpoint_id, "removed-key").await;
    let base = Utc::now() - Duration::minutes(5);
    record_observation(&pool, endpoint_id, live_key, base, true).await;
    record_observation(
        &pool,
        endpoint_id,
        removed_key,
        base + Duration::seconds(1),
        true,
    )
    .await;
    // The key leaves the current configuration, but its observations carry
    // no foreign key and must remain endpoint history.
    sqlx::query("DELETE FROM endpoint_api_keys WHERE key_id = $1")
        .bind(removed_key)
        .execute(&pool)
        .await
        .expect("remove only the owned test key");

    let body = body_json(
        get_history(
            &state,
            headers.clone(),
            endpoint_id,
            QuotaKeyObservationHistoryQuery::default(),
        )
        .await,
    )
    .await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["key_id"], removed_key.to_string());
    assert_eq!(items[0]["key_label"], "handler-key");
    assert_eq!(items[1]["key_id"], live_key.to_string());

    // The removed key is still directly addressable by UUID.
    let filtered = body_json(
        get_history(
            &state,
            headers,
            endpoint_id,
            QuotaKeyObservationHistoryQuery {
                key_id: Some(removed_key.to_string()),
                limit: None,
                before_id: None,
            },
        )
        .await,
    )
    .await;
    let filtered_items = filtered["items"].as_array().expect("items array");
    assert_eq!(filtered_items.len(), 1);
    assert_eq!(filtered_items[0]["key_id"], removed_key.to_string());
    cleanup(&pool, endpoint_id).await;
}

async fn body_json_large(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("large body reads");
    serde_json::from_slice(&bytes).expect("large body is JSON")
}

fn parse_point_at(point: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(point["at"].as_str().expect("point has an at timestamp"))
        .expect("point at parses")
        .with_timezone(&Utc)
}

#[tokio::test]
async fn trend_requires_an_admin_session_before_storage() {
    let endpoint_id = Uuid::new_v4();
    let unauthenticated = get_trend(
        &lazy_test_state(),
        axum::http::HeaderMap::new(),
        endpoint_id,
        QuotaKeyObservationTrendQuery::default(),
    )
    .await;
    assert_eq!(
        unauthenticated.status(),
        axum::http::StatusCode::UNAUTHORIZED
    );

    let state = lazy_test_state();
    attach_session(&state, "quota-trend-non-admin", false).await;
    let forbidden = get_trend(
        &state,
        admin_headers("quota-trend-non-admin"),
        endpoint_id,
        QuotaKeyObservationTrendQuery::default(),
    )
    .await;
    assert_eq!(forbidden.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn trend_rejects_unknown_range_and_bad_key_before_storage() {
    let state = lazy_test_state();
    attach_session(&state, "quota-trend-admin", true).await;
    let headers = admin_headers("quota-trend-admin");
    for query in [
        QuotaKeyObservationTrendQuery {
            key_id: None,
            range: Some("1h".to_string()),
        },
        QuotaKeyObservationTrendQuery {
            key_id: None,
            range: Some("".to_string()),
        },
        QuotaKeyObservationTrendQuery {
            key_id: Some("not-a-uuid".to_string()),
            range: None,
        },
    ] {
        let response = get_trend(&state, headers.clone(), Uuid::new_v4(), query).await;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn trend_rejects_missing_and_plain_provider_endpoints() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-trend-guard", true).await;
    let headers = admin_headers("quota-trend-guard");
    let generic_id = create_endpoint(&pool, "generic").await;

    let missing = get_trend(
        &state,
        headers.clone(),
        Uuid::new_v4(),
        QuotaKeyObservationTrendQuery::default(),
    )
    .await;
    assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
    let plain = get_trend(
        &state,
        headers,
        generic_id,
        QuotaKeyObservationTrendQuery::default(),
    )
    .await;
    assert_eq!(plain.status(), axum::http::StatusCode::NOT_FOUND);
    cleanup(&pool, generic_id).await;
}

#[tokio::test]
async fn trend_returns_bounded_series_with_failed_bucket() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-trend-reader", true).await;
    let headers = admin_headers("quota-trend-reader");
    let endpoint_id = create_endpoint(&pool, "opencode_go").await;
    let key_id = create_key(&pool, endpoint_id, "trend-key").await;
    let now = Utc::now();
    // Far enough apart to land in different 15-minute buckets.
    record_observation(
        &pool,
        endpoint_id,
        key_id,
        now - Duration::minutes(25),
        true,
    )
    .await;
    record_observation(
        &pool,
        endpoint_id,
        key_id,
        now - Duration::minutes(2),
        false,
    )
    .await;

    let body = body_json(
        get_trend(
            &state,
            headers,
            endpoint_id,
            QuotaKeyObservationTrendQuery::default(),
        )
        .await,
    )
    .await;
    assert_eq!(body["range"], "24h");
    assert_eq!(body["bucket_seconds"], 900);
    assert_eq!(body["bucket_count"], 96);
    assert_eq!(body["keys_truncated"], false);
    assert_eq!(body["series_truncated"], false);
    assert_eq!(body["sampled_observations"], 2);
    assert_eq!(body["failed_observations"], 1);

    let series = body["series"].as_array().expect("series array");
    assert_eq!(series.len(), 1);
    let rolling = &series[0];
    assert_eq!(rolling["source_id"], "opencode_go:rolling");
    assert_eq!(rolling["kind"], "rolling");
    assert_eq!(rolling["metric"], "remaining_percent");
    assert_eq!(rolling["unit"], "percent");
    let points = rolling["points"].as_array().expect("points array");
    assert_eq!(points.len(), 96);
    let from = DateTime::parse_from_rfc3339(body["from"].as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(parse_point_at(&points[0]), from);
    let mut previous = None;
    let mut numeric = 0;
    let mut observed = 0;
    for point in points {
        let at = parse_point_at(point);
        if let Some(previous) = previous {
            assert!(at > previous, "points ascend strictly");
        }
        previous = Some(at);
        if point["value"].is_number() {
            numeric += 1;
            assert_eq!(point["value"], 80.0);
        }
        if point["observed_at"].is_string() {
            observed += 1;
        }
    }
    assert_eq!(numeric, 1, "only the successful snapshot has a value");
    assert_eq!(
        observed, 2,
        "both the successful and the failed snapshot are sampled"
    );
    // The failed latest snapshot blanks its bucket without carrying a value.
    let failed = points
        .iter()
        .find(|point| point["observed_at"].is_string() && point["value"].is_null())
        .expect("failed bucket keeps an observed_at with a null value");
    assert!(failed["value"].is_null());
    cleanup(&pool, endpoint_id).await;
}

#[tokio::test]
async fn trend_reports_key_truncation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let state = test_state_with_pool(pool.clone());
    attach_session(&state, "quota-trend-cap", true).await;
    let headers = admin_headers("quota-trend-cap");
    let endpoint_id = create_endpoint(&pool, "opencode_go").await;
    let now = Utc::now();
    for index in 0..25 {
        let key_id = create_key(&pool, endpoint_id, &format!("key-{index}")).await;
        record_observation(
            &pool,
            endpoint_id,
            key_id,
            now - Duration::minutes(5 + index),
            true,
        )
        .await;
    }

    let body = body_json_large(
        get_trend(
            &state,
            headers,
            endpoint_id,
            QuotaKeyObservationTrendQuery::default(),
        )
        .await,
    )
    .await;
    assert_eq!(body["keys_truncated"], true);
    assert_eq!(body["series_truncated"], false);
    assert_eq!(body["sampled_observations"], 24);
    assert_eq!(body["series"].as_array().unwrap().len(), 24);
    cleanup(&pool, endpoint_id).await;
}
