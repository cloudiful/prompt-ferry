#[path = "support/db_harness.rs"]
mod db_harness;

use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db::{
    OverviewBucket, OverviewWindow, RequestRecordCategory, RequestRecordOverviewResponse,
    request_record_summary, request_records_overview, usage_buckets,
};
use prompt_ferry::{db, keys::hash_password};
use sqlx::PgPool;
use uuid::Uuid;

fn window() -> OverviewWindow {
    OverviewWindow {
        start: None,
        end: None,
        bucket: OverviewBucket::Hour,
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

#[derive(Clone, Copy)]
struct Tokens {
    input: i64,
    output: i64,
    total: i64,
    cache_read: i64,
}

/// `input` already holds `cache_read`, `total` is the persisted closed loop
/// (`input + output`), so re-adding the cache meters would double-count.
const FOLDED: Tokens = Tokens {
    input: 1_000,
    output: 100,
    total: 1_100,
    cache_read: 800,
};

/// Ordinary input only; `total == input + cache_read + output` already.
const NORMALIZED: Tokens = Tokens {
    input: 200,
    output: 50,
    total: 350,
    cache_read: 100,
};

async fn insert_ai_row(
    pool: &PgPool,
    user_id: i64,
    model: &str,
    tokens: Tokens,
) -> anyhow::Result<()> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(Some(user_id), None, None, None)
            .with_model(Some(model.to_string()))
            .with_timing(Some(200), Some(true), Some(1000), Some(50))
            .with_usage(
                Some(tokens.input),
                Some(tokens.output),
                Some(tokens.total),
                None,
                Some(tokens.cache_read),
                Some(0),
            ),
    )
    .await?;
    Ok(())
}

async fn insert_mcp_row(
    pool: &PgPool,
    user_id: i64,
    server_name: &str,
    tokens: Tokens,
) -> anyhow::Result<()> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::mcp_request(Uuid::new_v4(), "/mcp")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(Some(user_id), None, None, None)
            .with_mcp_context(
                None,
                Some(server_name.to_string()),
                Some("tools/call".to_string()),
                None,
            )
            .with_timing(Some(200), Some(true), Some(1000), None)
            .with_usage(
                Some(tokens.input),
                Some(tokens.output),
                Some(tokens.total),
                None,
                Some(tokens.cache_read),
                Some(0),
            ),
    )
    .await?;
    Ok(())
}

async fn overview(
    pool: &PgPool,
    category: RequestRecordCategory,
    login: &str,
) -> anyhow::Result<RequestRecordOverviewResponse> {
    request_records_overview(pool, None, category, window(), Some(login)).await
}

fn trend_total_tokens(response: &RequestRecordOverviewResponse) -> i64 {
    response
        .trend
        .iter()
        .map(|row| row.tokens.total_tokens)
        .sum()
}

fn breakdown_total_tokens(response: &RequestRecordOverviewResponse) -> i64 {
    response
        .breakdown
        .iter()
        .map(|row| row.tokens.total_tokens)
        .sum()
}

async fn buckets_total_tokens(
    pool: &PgPool,
    category: RequestRecordCategory,
) -> anyhow::Result<i64> {
    let buckets = usage_buckets(pool, "hour", 24, None, None, None, Some(category)).await?;
    Ok(buckets.iter().map(|bucket| bucket.total_tokens).sum())
}

/// A folded historical row must contribute its persisted `total_tokens`
/// once; the old per-row `ordinary + read + write + output` sum would have
/// reported 1900 instead of 1100.
#[tokio::test]
async fn folded_row_total_tokens_uses_persisted_closed_loop_value() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "overview-total-folded-342";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(&schema.pool, actor.user_id, "model-folded-342", FOLDED).await?;

    let response = overview(&schema.pool, RequestRecordCategory::Ai, login).await?;

    assert_eq!(response.summary.tokens.total_tokens, 1_100);
    assert_eq!(response.summary.tokens.input_tokens, 1_000);
    assert_eq!(response.summary.tokens.cache_read_tokens, 800);
    assert_eq!(response.summary.tokens.output_tokens, 100);
    assert_eq!(trend_total_tokens(&response), 1_100);
    assert_eq!(breakdown_total_tokens(&response), 1_100);

    let summary = request_record_summary(&schema.pool, 1, None).await?;
    assert_eq!(summary.total_tokens, 1_100);
    assert_eq!(
        buckets_total_tokens(&schema.pool, RequestRecordCategory::Ai).await?,
        1_100
    );

    schema.cleanup().await?;
    Ok(())
}

/// A normalized row's persisted total already equals the closed loop, so the
/// unified aggregation must keep it unchanged.
#[tokio::test]
async fn normalized_row_total_tokens_stays_closed_loop() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "overview-total-normalized-342";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "model-normalized-342",
        NORMALIZED,
    )
    .await?;

    let response = overview(&schema.pool, RequestRecordCategory::Ai, login).await?;

    assert_eq!(response.summary.tokens.total_tokens, 350);
    assert_eq!(trend_total_tokens(&response), 350);
    assert_eq!(breakdown_total_tokens(&response), 350);

    let summary = request_record_summary(&schema.pool, 1, None).await?;
    assert_eq!(summary.total_tokens, 350);
    assert_eq!(
        buckets_total_tokens(&schema.pool, RequestRecordCategory::Ai).await?,
        350
    );

    schema.cleanup().await?;
    Ok(())
}

/// Mixed folded + normalized rows must sum persisted totals (1450), not the
/// naive per-row recompute (2250); summary, trend and breakdown agree and the
/// per-meter sums stay untouched.
#[tokio::test]
async fn mixed_rows_sum_persisted_totals_across_overview_views() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "overview-total-mixed-342";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(&schema.pool, actor.user_id, "model-folded-342", FOLDED).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "model-normalized-342",
        NORMALIZED,
    )
    .await?;

    let response = overview(&schema.pool, RequestRecordCategory::Ai, login).await?;

    let expected_total = 1_100 + 350;
    assert_eq!(response.summary.tokens.total_tokens, expected_total);
    assert_eq!(trend_total_tokens(&response), expected_total);
    assert_eq!(breakdown_total_tokens(&response), expected_total);
    // The naive per-row closed-loop recompute would be 1900 + 350 = 2250.
    assert_ne!(trend_total_tokens(&response), 2_250);
    // Per-meter sums are still the raw normalized meters.
    assert_eq!(response.summary.tokens.input_tokens, 1_200);
    assert_eq!(response.summary.tokens.cache_read_tokens, 900);
    assert_eq!(response.summary.tokens.output_tokens, 150);

    let summary = request_record_summary(&schema.pool, 1, None).await?;
    assert_eq!(summary.total_tokens, expected_total);
    assert_eq!(
        buckets_total_tokens(&schema.pool, RequestRecordCategory::Ai).await?,
        expected_total
    );

    schema.cleanup().await?;
    Ok(())
}

/// `token_share` and the model ordering must follow the same persisted totals
/// so a folded model is not inflated (1100/1450, not 1900/2250).
#[tokio::test]
async fn ai_breakdown_token_share_uses_persisted_totals() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "overview-total-share-342";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(&schema.pool, actor.user_id, "model-folded-342", FOLDED).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "model-normalized-342",
        NORMALIZED,
    )
    .await?;

    let response = overview(&schema.pool, RequestRecordCategory::Ai, login).await?;
    let rows = &response.breakdown;
    assert_eq!(rows[0].label, "model-folded-342");

    let folded = rows
        .iter()
        .find(|row| row.label == "model-folded-342")
        .expect("folded model row");
    let normalized = rows
        .iter()
        .find(|row| row.label == "model-normalized-342")
        .expect("normalized model row");
    assert_eq!(folded.tokens.total_tokens, 1_100);
    assert_eq!(normalized.tokens.total_tokens, 350);
    assert_ratio(folded.token_share, 1_100.0 / 1_450.0);
    assert_ratio(normalized.token_share, 350.0 / 1_450.0);
    let share_sum = folded.token_share.unwrap() + normalized.token_share.unwrap();
    assert!((share_sum - 1.0).abs() < 1e-9, "shares sum to {share_sum}");

    schema.cleanup().await?;
    Ok(())
}

/// The grouped MCP breakdown exit must also read the persisted total and stop
/// re-deriving it from the sum of the meter columns.
#[tokio::test]
async fn mcp_breakdown_total_tokens_uses_persisted_value() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "overview-total-mcp-342";
    let actor = create_actor(&schema.pool, login).await?;
    let server = "overview-mcp-server-342";
    insert_mcp_row(&schema.pool, actor.user_id, server, FOLDED).await?;

    let response = overview(&schema.pool, RequestRecordCategory::Mcp, login).await?;
    let row = response
        .breakdown
        .iter()
        .find(|row| row.label == server)
        .expect("MCP breakdown row");

    assert_eq!(response.summary.tokens.total_tokens, 1_100);
    assert_eq!(trend_total_tokens(&response), 1_100);
    assert_eq!(row.tokens.total_tokens, 1_100);
    assert_eq!(row.token_share, None);

    let summary = request_record_summary(&schema.pool, 1, None).await?;
    assert_eq!(summary.total_tokens, 1_100);
    assert_eq!(
        buckets_total_tokens(&schema.pool, RequestRecordCategory::Mcp).await?,
        1_100
    );

    schema.cleanup().await?;
    Ok(())
}

fn assert_ratio(actual: Option<f64>, expected: f64) {
    let value = actual.expect("token_share must be present");
    assert!(
        (value - expected).abs() < 1e-9,
        "token_share {value} != expected {expected}"
    );
}
