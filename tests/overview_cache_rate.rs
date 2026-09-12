#[path = "support/db_harness.rs"]
mod db_harness;

use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::{db, keys::hash_password};
use sqlx::PgPool;
use uuid::Uuid;

use prompt_ferry::db::{
    OverviewBucket, OverviewWindow, RequestRecordCategory, RequestRecordOverviewResponse,
    request_records_overview,
};

fn window(bucket: OverviewBucket) -> OverviewWindow {
    OverviewWindow {
        start: None,
        end: None,
        bucket,
    }
}

fn assert_rate(actual: Option<f64>, expected: f64) {
    let value = actual.expect("cache_rate must be present");
    assert!(
        (value - expected).abs() < 1e-9,
        "cache_rate {value} != expected {expected}"
    );
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

struct Tokens {
    input: i64,
    output: i64,
    total: i64,
    cache_read: i64,
}

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
    bucket: OverviewBucket,
    login: &str,
) -> anyhow::Result<RequestRecordOverviewResponse> {
    request_records_overview(pool, None, category, window(bucket), Some(login)).await
}

/// A still-folded historical row stores the cache inside `input_tokens`
/// (`input >= total - output`), so the `cache_rate` denominator is
/// `max(input, read+write)` per row and the trend must not re-add the cache.
#[tokio::test]
async fn trend_cache_rate_matches_summary_for_still_folded_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-folded-338";
    let actor = create_actor(&schema.pool, login).await?;
    // input=1000 already holds cache_read=800; total=input+output=1100.
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "gpt-folded-338",
        Tokens {
            input: 1000,
            output: 100,
            total: 1100,
            cache_read: 800,
        },
    )
    .await?;

    let response = overview(
        &schema.pool,
        RequestRecordCategory::Ai,
        OverviewBucket::Hour,
        login,
    )
    .await?;

    // Without the fold-aware SQL denominator this would be 800/(1000+800)=0.444.
    assert_rate(response.summary.tokens.cache_rate, 800.0 / 1000.0);
    let bucket = response.trend.first().expect("trend bucket");
    assert_rate(bucket.tokens.cache_rate, 800.0 / 1000.0);
    assert_rate(response.summary.tokens.cache_hit_rate, 1.0);

    schema.cleanup().await?;
    Ok(())
}

/// A post-backfill row stores ordinary input only, so the denominator is
/// `ordinary + read + write`; trend and summary must agree.
#[tokio::test]
async fn trend_cache_rate_uses_ordinary_plus_cache_for_normalized_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-normalized-338";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "gpt-normalized-338",
        Tokens {
            input: 200,
            output: 50,
            total: 350,
            cache_read: 100,
        },
    )
    .await?;

    let response = overview(
        &schema.pool,
        RequestRecordCategory::Ai,
        OverviewBucket::Day,
        login,
    )
    .await?;

    assert_rate(response.summary.tokens.cache_rate, 100.0 / 300.0);
    let bucket = response.trend.first().expect("trend bucket");
    assert_rate(bucket.tokens.cache_rate, 100.0 / 300.0);

    schema.cleanup().await?;
    Ok(())
}

/// Aggregate `cache_rate` must weight by the per-row fold-aware denominator:
/// summary, trend and MCP breakdown share `SUM(read) / SUM(full_input)`, not a
/// raw input SUM and not the unweighted mean of per-row rates.
#[tokio::test]
async fn overview_and_mcp_breakdown_weight_aggregate_by_full_input() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-weighted-338";
    let actor = create_actor(&schema.pool, login).await?;
    let server = "agg-server-338";
    insert_mcp_row(
        &schema.pool,
        actor.user_id,
        server,
        Tokens {
            input: 1000,
            output: 100,
            total: 1100,
            cache_read: 800,
        },
    )
    .await?;
    insert_mcp_row(
        &schema.pool,
        actor.user_id,
        server,
        Tokens {
            input: 200,
            output: 50,
            total: 350,
            cache_read: 100,
        },
    )
    .await?;

    let response = overview(
        &schema.pool,
        RequestRecordCategory::Mcp,
        OverviewBucket::Day,
        login,
    )
    .await?;

    // full_input = max(1000,800) + (200+100) = 1300; read = 900.
    let expected = 900.0 / 1300.0;
    assert_rate(response.summary.tokens.cache_rate, expected);
    let bucket = response.trend.first().expect("trend bucket");
    assert_rate(bucket.tokens.cache_rate, expected);

    let breakdown = &response.breakdown;
    let row = breakdown
        .iter()
        .find(|row| row.label == server)
        .expect("MCP breakdown row");
    assert_rate(row.tokens.cache_rate, expected);

    // Raw SUM(input)+SUM(read) would report 900/2100; the fold guard must win.
    assert!((expected - 900.0 / 2100.0).abs() > 0.1);

    schema.cleanup().await?;
    Ok(())
}

/// A window whose rows carry no tokens must report no rate rather than a
/// fabricated one.
#[tokio::test]
async fn overview_cache_rate_is_none_when_denominator_is_empty() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-empty-338";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "gpt-empty-338",
        Tokens {
            input: 0,
            output: 0,
            total: 0,
            cache_read: 0,
        },
    )
    .await?;

    let response = overview(
        &schema.pool,
        RequestRecordCategory::Ai,
        OverviewBucket::Day,
        login,
    )
    .await?;

    assert_eq!(response.summary.tokens.cache_rate, None);
    let bucket = response.trend.first().expect("trend bucket");
    assert_eq!(bucket.tokens.cache_rate, None);

    schema.cleanup().await?;
    Ok(())
}
