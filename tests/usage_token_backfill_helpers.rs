//! Shared helpers for the backfill integration tests. Loaded via
//! `#[path = "..."] mod helpers;` from `tests/usage_token_backfill.rs` so
//! the test crate can stay small.

use prompt_ferry::db::{
    self, BackfillBatchOutcome, BackfillOptions, RequestRecordCreate, RequestRecordState,
    UsageEventKind,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TokenSnapshot {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
}

pub const ANTHROPIC_SSE_RESPONSE: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0}}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":176,\"output_tokens\":42,\"cache_read_input_tokens\":82793,\"cache_creation_input_tokens\":7}}\n\n";

pub const PARTIAL_SSE_RESPONSE: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_partial\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":0}}}\n\n";

pub async fn insert_old_anthropic(
    pool: &sqlx::PgPool,
    path: &str,
    response_body: &str,
) -> anyhow::Result<i64> {
    let event_id = db::record_request_record(
        pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), path)
            .with_state(UsageEventKind::Request, RequestRecordState::Completed)
            .with_model(Some("public-model".to_string()))
            .with_billing_models(
                Some("public-model".to_string()),
                Some("upstream-model".to_string()),
            )
            .with_timing(Some(200), Some(true), Some(50), Some(5))
            .with_usage(Some(176), Some(42), Some(82969), Some(82793), None, None),
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_backfill/insert_request_record_raw_payload.sql",
        event_id,
        Option::<serde_json::Value>::None,
        Some(response_body.to_string()),
    )
    .execute(pool)
    .await?;
    Ok(event_id)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_completed_with_tokens(
    pool: &sqlx::PgPool,
    path: &str,
    input: i64,
    output: i64,
    cache_read: Option<i64>,
    cache_write: Option<i64>,
    truncated: bool,
    raw_body: Option<&str>,
) -> anyhow::Result<i64> {
    let event_id = db::record_request_record(
        pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), path)
            .with_state(UsageEventKind::Request, RequestRecordState::Completed)
            .with_model(Some("public-model".to_string()))
            .with_billing_models(
                Some("public-model".to_string()),
                Some("upstream-model".to_string()),
            )
            .with_timing(Some(200), Some(true), Some(50), Some(5))
            .with_usage(
                Some(input),
                Some(output),
                Some(input + output),
                None,
                cache_read,
                cache_write,
            ),
    )
    .await?;
    if let Some(body) = raw_body {
        sqlx::query_file!(
            "tests/sql/usage_backfill/insert_request_record_raw_payload.sql",
            event_id,
            Option::<serde_json::Value>::None,
            Some(body.to_string()),
        )
        .execute(pool)
        .await?;
    }
    if truncated {
        sqlx::query_file!(
            "tests/sql/usage_backfill/set_response_capture_truncated.sql",
            event_id,
            true,
        )
        .execute(pool)
        .await?;
    }
    Ok(event_id)
}

pub async fn stored_tokens(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<TokenSnapshot> {
    let row = sqlx::query_file_as!(
        TokenSnapshot,
        "tests/sql/usage_backfill/get_request_record_tokens.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn run_batch(pool: &sqlx::PgPool, options: BackfillOptions) -> BackfillBatchOutcome {
    db::backfill_token_usage(pool, options)
        .await
        .expect("backfill_token_usage should succeed")
}
