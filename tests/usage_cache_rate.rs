#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db::{
    self, RequestRecordBucket, RequestRecordCategory, RequestRecordSummary, request_record_summary,
    usage_buckets,
};
use prompt_ferry::keys::hash_password;
use sqlx::PgPool;
use uuid::Uuid;

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

async fn summary_for(pool: &PgPool, user_id: i64) -> anyhow::Result<RequestRecordSummary> {
    request_record_summary(pool, 1, Some(user_id)).await
}

/// Fetch the single bucket that actually holds the inserted rows. The series
/// also yields empty leading buckets, so select on `request_count` rather than
/// on position.
async fn bucket_for(
    pool: &PgPool,
    bucket: &str,
    user_id: i64,
) -> anyhow::Result<RequestRecordBucket> {
    let now = Utc::now();
    let rows = usage_buckets(
        pool,
        bucket,
        10,
        Some(now - Duration::minutes(2)),
        Some(now),
        Some(user_id),
        Some(RequestRecordCategory::Ai),
    )
    .await?;
    Ok(rows
        .into_iter()
        .find(|row| row.request_count > 0)
        .expect("a bucket with rows"))
}

const BUCKETS: [&str; 3] = ["minute", "hour", "day"];

/// A post-backfill row stores ordinary input only, so the denominator is
/// `ordinary + read + write`; summary and every bucket must agree with the
/// overview canonical guard instead of the legacy `max(input, cache_sum)`.
#[tokio::test]
async fn summary_and_buckets_use_ordinary_plus_cache_for_normalized_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-summary-normalized-338";
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

    // Legacy `GREATEST(input, cache_sum)` reported 100/200 = 0.5.
    let expected = 100.0 / 300.0;
    assert_rate(
        summary_for(&schema.pool, actor.user_id).await?.cache_rate,
        expected,
    );
    for bucket in BUCKETS {
        assert_rate(
            bucket_for(&schema.pool, bucket, actor.user_id)
                .await?
                .cache_rate,
            expected,
        );
    }

    schema.cleanup().await?;
    Ok(())
}

/// A still-folded historical row stores the cache inside `input_tokens`, so the
/// denominator stays `max(input, read+write)` and the cache must not be added
/// on top of the input that already holds it.
#[tokio::test]
async fn summary_and_buckets_keep_folded_rows_on_max_denominator() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-summary-folded-338";
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

    // Naively summing the cache on top of the folded input would give
    // 800/1800 = 0.444.
    let expected = 800.0 / 1000.0;
    assert_rate(
        summary_for(&schema.pool, actor.user_id).await?.cache_rate,
        expected,
    );
    for bucket in BUCKETS {
        assert_rate(
            bucket_for(&schema.pool, bucket, actor.user_id)
                .await?
                .cache_rate,
            expected,
        );
    }

    schema.cleanup().await?;
    Ok(())
}

/// The aggregate rate must weight by the per-row fold-aware denominator:
/// `SUM(read) / SUM(full_input)`, not a raw input SUM.
#[tokio::test]
async fn summary_and_buckets_weight_aggregate_by_full_input() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-summary-weighted-338";
    let actor = create_actor(&schema.pool, login).await?;
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

    // full_input = max(1000,800) + (200+100) = 1300; read = 900.
    let expected = 900.0 / 1300.0;
    assert_rate(
        summary_for(&schema.pool, actor.user_id).await?.cache_rate,
        expected,
    );
    for bucket in BUCKETS {
        assert_rate(
            bucket_for(&schema.pool, bucket, actor.user_id)
                .await?
                .cache_rate,
            expected,
        );
    }
    // A raw `SUM(input)` denominator would report 900/2100.
    assert!((expected - 900.0 / 2100.0).abs() > 0.1);

    schema.cleanup().await?;
    Ok(())
}

/// Rows that carry no tokens have no valid denominator, so the summary and the
/// occupied bucket report no rate rather than a fabricated zero.
#[tokio::test]
async fn summary_and_buckets_report_no_rate_without_a_denominator() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-summary-empty-338";
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

    assert_eq!(
        summary_for(&schema.pool, actor.user_id).await?.cache_rate,
        None
    );
    for bucket in BUCKETS {
        assert_eq!(
            bucket_for(&schema.pool, bucket, actor.user_id)
                .await?
                .cache_rate,
            None
        );
    }

    schema.cleanup().await?;
    Ok(())
}

/// A window with no rows (empty summary range and an empty bucket series) must
/// still report no rate.
#[tokio::test]
async fn summary_and_buckets_report_no_rate_for_empty_windows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let login = "cache-rate-summary-window-338";
    let actor = create_actor(&schema.pool, login).await?;
    insert_ai_row(
        &schema.pool,
        actor.user_id,
        "gpt-window-338",
        Tokens {
            input: 200,
            output: 50,
            total: 350,
            cache_read: 100,
        },
    )
    .await?;

    let empty_summary = request_record_summary(&schema.pool, 0, Some(actor.user_id)).await?;
    assert_eq!(empty_summary.request_count, 0);
    assert_eq!(empty_summary.cache_rate, None);

    let now = Utc::now();
    let empty_buckets = usage_buckets(
        &schema.pool,
        "day",
        10,
        Some(now + Duration::days(1)),
        Some(now + Duration::days(2)),
        Some(actor.user_id),
        Some(RequestRecordCategory::Ai),
    )
    .await?;
    assert!(empty_buckets.iter().all(|row| row.request_count == 0));
    assert!(empty_buckets.iter().all(|row| row.cache_rate.is_none()));

    schema.cleanup().await?;
    Ok(())
}
