use super::{HistoryQueryError, parse_history_query, quota_snapshot_history};
use crate::{
    db,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        QuotaSnapshotHistoryQuery, RequestContentLoggingMode, RequestContentLoggingResponse,
        SessionUser, UsageRetentionSettings,
    },
};
use axum::http::header;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

fn test_state() -> AdminState {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://quota_history_test@127.0.0.1:1/quota_history_test")
        .expect("lazy test pool");
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
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(Duration::from_secs(
            60,
        )),
    })
}

async fn attach_session(state: &AdminState, id: &str, is_admin: bool) {
    state
        .replay_cache
        .write_session(
            id,
            &SessionUser {
                user_id: 1,
                login_name: "quota-history-test".to_string(),
                display_name: "Quota history test".to_string(),
                is_admin,
            },
        )
        .await
        .expect("test session");
}

#[test]
fn history_query_defaults_and_enforces_bounded_pages() {
    assert_eq!(
        parse_history_query(QuotaSnapshotHistoryQuery::default()).unwrap(),
        (50, None)
    );
    assert_eq!(
        parse_history_query(QuotaSnapshotHistoryQuery {
            limit: Some(1),
            before_id: None,
        })
        .unwrap(),
        (1, None)
    );
    assert_eq!(
        parse_history_query(QuotaSnapshotHistoryQuery {
            limit: Some(200),
            before_id: None,
        })
        .unwrap(),
        (200, None)
    );
    for limit in [0, 201] {
        assert_eq!(
            parse_history_query(QuotaSnapshotHistoryQuery {
                limit: Some(limit),
                before_id: None,
            }),
            Err(HistoryQueryError::InvalidLimit)
        );
    }
}

#[test]
fn history_cursor_requires_a_positive_i64_snapshot_id() {
    assert_eq!(
        parse_history_query(QuotaSnapshotHistoryQuery {
            limit: None,
            before_id: Some(i64::MAX.to_string()),
        })
        .unwrap(),
        (50, Some(i64::MAX))
    );
    for before_id in ["0", "-1", "1.5", " 1", "9223372036854775808"] {
        assert_eq!(
            parse_history_query(QuotaSnapshotHistoryQuery {
                limit: None,
                before_id: Some(before_id.to_string()),
            }),
            Err(HistoryQueryError::InvalidCursor),
            "cursor={before_id}"
        );
    }
}

#[tokio::test]
async fn history_requires_an_admin_session_before_accessing_storage() {
    let endpoint_id = Uuid::new_v4();
    let unauthenticated = quota_snapshot_history(
        axum::extract::State(test_state()),
        axum::http::HeaderMap::new(),
        axum::extract::Path(endpoint_id),
        axum::extract::Query(QuotaSnapshotHistoryQuery::default()),
    )
    .await;
    assert_eq!(
        unauthenticated.status(),
        axum::http::StatusCode::UNAUTHORIZED
    );

    let state = test_state();
    attach_session(&state, "quota-history-non-admin", false).await;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        "prompt_ferry_session=quota-history-non-admin"
            .parse()
            .expect("session cookie"),
    );
    let forbidden = quota_snapshot_history(
        axum::extract::State(state),
        headers,
        axum::extract::Path(endpoint_id),
        axum::extract::Query(QuotaSnapshotHistoryQuery::default()),
    )
    .await;
    assert_eq!(forbidden.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn invalid_history_parameters_are_rejected_before_storage_access() {
    let state = test_state();
    attach_session(&state, "quota-history-admin", true).await;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        "prompt_ferry_session=quota-history-admin"
            .parse()
            .expect("session cookie"),
    );
    for query in [
        QuotaSnapshotHistoryQuery {
            limit: Some(201),
            before_id: None,
        },
        QuotaSnapshotHistoryQuery {
            limit: None,
            before_id: Some("0".to_string()),
        },
    ] {
        let response = quota_snapshot_history(
            axum::extract::State(state.clone()),
            headers.clone(),
            axum::extract::Path(Uuid::new_v4()),
            axum::extract::Query(query),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
}
