use chrono::{Duration, Utc};
use prompt_ferry::{
    config::{NativeApi, NativeApiSource},
    db,
    keys::hash_password,
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::db_harness::TestSchema;

pub async fn migrated_schema() -> anyhow::Result<TestSchema> {
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    Ok(schema)
}

pub async fn create_test_user(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    Ok(db::create_user(
        pool,
        db::UserCreate {
            login_name: "billing-test-admin".to_string(),
            password_hash: hash_password("billing-test-password")?,
            display_name: "Billing Test Admin".to_string(),
            is_admin: true,
        },
    )
    .await?
    .user_id)
}

pub async fn create_test_endpoint(pool: &sqlx::PgPool) -> anyhow::Result<Uuid> {
    Ok(db::create_endpoint(
        pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "billing-test-endpoint".to_string(),
            base_url: "https://billing-test.invalid".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "billing-test-key".to_string(),
            api_keys: Vec::new(),
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?
    .endpoint_id)
}

pub async fn create_unpriced_charge(pool: &sqlx::PgPool, endpoint_id: Uuid) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/chat/completions")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_route(Some(endpoint_id), None)
            .with_model(Some("public-model".to_string()))
            .with_billing_models(
                Some("public-model".to_string()),
                Some("upstream-model".to_string()),
            )
            .with_timing(Some(200), Some(true), Some(50), Some(5))
            .with_usage(
                Some(1_000_000),
                Some(2_000_000),
                Some(3_000_000),
                None,
                None,
                None,
            ),
    )
    .await
}

pub async fn create_customer_price_rule(
    pool: &sqlx::PgPool,
    created_by_user_id: i64,
) -> anyhow::Result<Uuid> {
    let effective_from = Utc::now() - Duration::minutes(1);
    let rule = db::create_price_rule(
        pool,
        db::BillingPriceRuleCreate {
            public_model: "public-model".to_string(),
            input_rate: Decimal::ONE,
            cache_read_rate: Decimal::ZERO,
            cache_write_rate: Decimal::ZERO,
            output_rate: Decimal::from(4),
            effective_from,
            created_by_user_id,
        },
    )
    .await?;
    Ok(rule.price_rule_id)
}
