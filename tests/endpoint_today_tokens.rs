#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db,
};
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_endpoint(pool: &sqlx::PgPool, name: &str) -> anyhow::Result<Uuid> {
    Ok(db::create_endpoint(
        pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: name.to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("https://{name}.invalid"),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "test-key".to_string(),
            api_keys: Vec::new(),
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?
    .endpoint_id)
}

async fn record_ai_usage(
    pool: &sqlx::PgPool,
    endpoint_id: Uuid,
    total_tokens: i64,
) -> anyhow::Result<i64> {
    Ok(db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/chat/completions")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_route(Some(endpoint_id), None)
            .with_model(Some("test-model".to_string()))
            .with_timing(Some(200), Some(true), Some(10), Some(1))
            .with_usage(None, None, Some(total_tokens), None, None, None),
    )
    .await?)
}

#[tokio::test]
async fn endpoint_today_tokens_sums_only_current_endpoint_and_day() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let endpoint_a = create_endpoint(&schema.pool, "endpoint-a").await?;
    let endpoint_b = create_endpoint(&schema.pool, "endpoint-b").await?;

    record_ai_usage(&schema.pool, endpoint_a, 300).await?;
    record_ai_usage(&schema.pool, endpoint_b, 500).await?;
    let old = record_ai_usage(&schema.pool, endpoint_a, 100).await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        old,
        Utc::now() - Duration::days(2),
    )
    .execute(&schema.pool)
    .await?;

    let now = Utc::now();
    assert_eq!(
        db::endpoint_today_tokens(&schema.pool, endpoint_a, now).await?,
        300,
        "only the current UTC day and the requested endpoint count"
    );
    assert_eq!(
        db::endpoint_today_tokens(&schema.pool, endpoint_b, now).await?,
        500
    );

    schema.cleanup().await?;
    Ok(())
}
