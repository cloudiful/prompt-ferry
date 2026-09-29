#[path = "support/db_harness.rs"]
mod db_harness;

use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::config::{NativeApi, NativeApiSource};
use prompt_ferry::db::{
    OverviewBucket, OverviewWindow, RequestRecordCategory, RequestRecordOverviewBreakdownRow,
    RequestRecordOverviewResponse, request_records_overview,
};
use prompt_ferry::{db, keys::hash_password};
use sqlx::PgPool;
use uuid::Uuid;

fn window() -> OverviewWindow {
    OverviewWindow {
        start: None,
        end: None,
        bucket: OverviewBucket::Day,
    }
}

async fn create_actor(pool: &PgPool, login: &str) -> anyhow::Result<db::User> {
    db::create_user(
        pool,
        db::UserCreate {
            login_name: login.to_string(),
            password_hash: hash_password("password-123")?,
            display_name: login.to_string(),
            is_admin: true,
        },
    )
    .await
}

async fn create_upstream(pool: &PgPool, name: &str) -> anyhow::Result<db::ProviderEndpoint> {
    db::create_endpoint(
        pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: name.to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{name}.example.test"),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            api_key: format!("{name}-key"),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
            proxy_url: None,
            active_windows: None,
        },
    )
    .await
}

#[derive(Clone, Copy)]
struct Tokens {
    input: i64,
    output: i64,
    total: i64,
    cache_read: i64,
}

#[allow(clippy::too_many_arguments)]
async fn insert_ai_row(
    pool: &PgPool,
    user_id: i64,
    model: &str,
    upstream_model: Option<&str>,
    endpoint_id: Option<Uuid>,
    ok: bool,
    tokens: Tokens,
) -> anyhow::Result<()> {
    let (record_state, status) = if ok {
        (db::RequestRecordState::Completed, 200)
    } else {
        (db::RequestRecordState::Failed, 500)
    };
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(db::UsageEventKind::Request, record_state)
            .with_request_actor(Some(user_id), None, None, None)
            .with_route(endpoint_id, None)
            .with_model(Some(model.to_string()))
            .with_billing_models(Some(model.to_string()), upstream_model.map(str::to_string))
            .with_timing(Some(status), Some(ok), Some(1000), Some(50))
            .with_usage(
                Some(tokens.input),
                Some(tokens.output),
                Some(tokens.total),
                None,
                Some(tokens.cache_read),
                Some(0),
            )
            .with_failure_family(if ok {
                None
            } else {
                Some(db::RequestFailureFamily::Timeout)
            }),
    )
    .await?;
    Ok(())
}

async fn overview(pool: &PgPool, login: &str) -> anyhow::Result<RequestRecordOverviewResponse> {
    request_records_overview(pool, None, RequestRecordCategory::Ai, window(), Some(login)).await
}

fn model_row<'a>(
    response: &'a RequestRecordOverviewResponse,
    label: &str,
) -> &'a RequestRecordOverviewBreakdownRow {
    response
        .breakdown
        .iter()
        .find(|row| row.label == label)
        .unwrap_or_else(|| panic!("breakdown row {label} must exist"))
}

/// Issue #593: every upstream-model combination is a separate entry carrying
/// the main table's seven metrics, so a route override is distinguishable.
#[tokio::test]
async fn upstream_breakdown_splits_requested_endpoint_and_actual_model() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "upstream-metrics-593";
    let actor = create_actor(&schema.pool, login).await?;

    let endpoint_a = create_upstream(&schema.pool, "upstream-a-593").await?;
    let endpoint_b = create_upstream(&schema.pool, "upstream-b-593").await?;

    let model = "gpt-upstream-metrics-593";
    let override_model = "gpt-upstream-metrics-593-mini";
    let tokens = Tokens {
        input: 10,
        output: 100,
        total: 110,
        cache_read: 0,
    };
    // Same endpoint, once with and once without a route-model override.
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        model,
        None,
        Some(endpoint_a.endpoint_id),
        true,
        tokens,
    )
    .await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        model,
        Some(override_model),
        Some(endpoint_a.endpoint_id),
        true,
        tokens,
    )
    .await?;
    // Second endpoint: one success and one error, no override.
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        model,
        None,
        Some(endpoint_b.endpoint_id),
        true,
        tokens,
    )
    .await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        model,
        None,
        Some(endpoint_b.endpoint_id),
        false,
        tokens,
    )
    .await?;

    let response = overview(&schema.pool, login).await?;
    let row = model_row(&response, model);

    assert_eq!(
        row.upstream_count,
        Some(3),
        "each requested-model x endpoint x upstream-model combination is its own row"
    );
    let entries = row
        .upstream_breakdown
        .as_ref()
        .expect("AI model row must carry its upstream breakdown");
    assert_eq!(entries.len(), 3);

    for entry in entries {
        // All seven main-table metrics are present with their identity columns.
        assert!(entry.request_count > 0, "request_count must be positive");
        assert!(entry.request_share > 0.0, "request_share must be positive");
        assert!(entry.total_tokens > 0, "total_tokens must be positive");
        assert!(entry.token_share.is_some(), "token_share must be numeric");
        assert!(entry.cache_rate.is_some(), "cache_rate must be numeric");
        assert!(entry.error_rate >= 0.0, "error_rate must be numeric");
        assert!(
            entry.avg_output_tokens_per_second.is_some(),
            "avg_output_tokens_per_second must be present for completed rows"
        );
    }

    let direct = entries
        .iter()
        .find(|entry| {
            entry.upstream_model.is_none() && entry.endpoint_id == Some(endpoint_a.endpoint_id)
        })
        .expect("endpoint A without override must be its own entry");
    assert_eq!(direct.request_count, 1);

    let override_entry = entries
        .iter()
        .find(|entry| entry.upstream_model.as_deref() == Some(override_model))
        .expect("override request must surface the actual upstream model");
    assert_eq!(
        override_entry.endpoint_id,
        Some(endpoint_a.endpoint_id),
        "override entry stays paired with its endpoint"
    );
    assert_eq!(
        override_entry.endpoint_name.as_deref(),
        Some(endpoint_a.name.as_str())
    );
    assert_eq!(
        override_entry.request_count, 1,
        "override entry must only count the overridden request"
    );

    let endpoint_b_entry = entries
        .iter()
        .find(|entry| entry.endpoint_id == Some(endpoint_b.endpoint_id))
        .expect("endpoint B entry must exist");
    assert_eq!(endpoint_b_entry.request_count, 2);
    assert_eq!(endpoint_b_entry.error_count, 1);
    assert!((endpoint_b_entry.error_rate - 0.5).abs() < 1e-9);

    // Shares use the window totals, matching the main model table (4 requests).
    assert!((override_entry.request_share - 0.25).abs() < 1e-9);
    assert!((endpoint_b_entry.request_share - 0.5).abs() < 1e-9);
    assert!((endpoint_b_entry.token_share.expect("token share") - 0.5).abs() < 1e-9);

    schema.cleanup().await?;
    Ok(())
}

/// Issue #593: with a zero-token window the share denominators stay NULL,
/// exactly like the main model table's `token_share`.
#[tokio::test]
async fn upstream_token_share_is_null_without_total_tokens() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "upstream-zero-tokens-593";
    let actor = create_actor(&schema.pool, login).await?;

    let model = "gpt-upstream-zero-593";
    let zero = Tokens {
        input: 0,
        output: 0,
        total: 0,
        cache_read: 0,
    };
    insert_ai_row(&schema.pool, actor.user_id, model, None, None, true, zero).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        model,
        Some("gpt-upstream-zero-593-mini"),
        None,
        true,
        zero,
    )
    .await?;

    let response = overview(&schema.pool, login).await?;
    let row = model_row(&response, model);

    assert!(
        row.token_share.is_none(),
        "main row token_share must stay NULL without total tokens"
    );
    let entries = row
        .upstream_breakdown
        .as_ref()
        .expect("zero-token model must keep its upstream entries");
    assert!(!entries.is_empty());
    for entry in entries {
        assert!(
            entry.token_share.is_none(),
            "upstream token_share must stay NULL without total tokens"
        );
        assert!(
            entry.request_share > 0.0,
            "request_share must stay numeric when tokens are zero"
        );
        assert!(
            entry.cache_rate.is_none(),
            "cache_rate must stay NULL without a full-input denominator"
        );
    }

    schema.cleanup().await?;
    Ok(())
}
