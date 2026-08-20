//! Regression tests for backfill safety guards (truncated raw, missing raw,
//! diagnostics, cursor advance past Skipped/Failed, already-priced charge
//! with no remaining price rule). Loaded from `tests/usage_token_backfill.rs`
//! via `#[path = "..."] mod safety_tests;`.

#[path = "support/billing_harness.rs"]
mod billing_harness;
#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "usage_token_backfill_helpers.rs"]
mod helpers;

use billing_harness::migrated_schema;
use db_harness::{TEST_DATABASE_URL_ENV, test_database_configured};
use helpers::{
    ANTHROPIC_SSE_RESPONSE, PARTIAL_SSE_RESPONSE, insert_completed_with_tokens,
    insert_old_anthropic, run_batch, stored_tokens,
};
use prompt_ferry::db::{
    self, BackfillOptions, RequestRecordCreate, RequestRecordState, UsageEventKind,
};
use uuid::Uuid;

#[tokio::test]
async fn missing_raw_body_records_are_skipped() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let event_id = db::record_request_record(
        &schema.pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/messages")
            .with_state(UsageEventKind::Request, RequestRecordState::Completed)
            .with_model(Some("public-model".to_string()))
            .with_timing(Some(200), Some(true), Some(10), Some(1))
            .with_usage(Some(100), Some(20), Some(120), None, None, None),
    )
    .await?;
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert!(outcome.stats.skipped >= 1);
    let after = stored_tokens(&schema.pool, event_id).await?;
    assert_eq!(after.input_tokens, Some(100));
    assert_eq!(after.output_tokens, Some(20));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn failed_state_rows_are_excluded_from_candidate_scan() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let failed_event_id = db::record_request_record(
        &schema.pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/messages")
            .with_state(UsageEventKind::Request, RequestRecordState::Failed)
            .with_model(Some("public-model".to_string()))
            .with_timing(Some(500), Some(false), Some(50), None)
            .with_usage(Some(176), Some(0), Some(176), Some(82793), None, None),
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_backfill/insert_request_record_raw_payload.sql",
        failed_event_id,
        Option::<serde_json::Value>::None,
        Some(ANTHROPIC_SSE_RESPONSE.to_string()),
    )
    .execute(&schema.pool)
    .await?;
    let before = stored_tokens(&schema.pool, failed_event_id).await?;
    let _ = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        stored_tokens(&schema.pool, failed_event_id).await?,
        before,
        "failed rows must not be repaired or rewritten"
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn truncated_candidate_with_partial_sse_remains_unchanged() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let event_id = insert_completed_with_tokens(
        &schema.pool,
        "/v1/messages",
        82976,
        42,
        Some(82793),
        Some(7),
        true,
        Some(PARTIAL_SSE_RESPONSE),
    )
    .await?;
    let before = stored_tokens(&schema.pool, event_id).await?;
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        stored_tokens(&schema.pool, event_id).await?,
        before,
        "truncated rows must not be repaired or rewritten"
    );
    assert_eq!(
        outcome.stats.failed, 0,
        "truncated rows are skipped, not failed"
    );
    assert!(
        outcome.stats.skipped >= 1,
        "truncated rows must increment stats.skipped, got {:?}",
        outcome.stats
    );
    let truncated_diag = outcome
        .diagnostics
        .iter()
        .find(|d| d.event_id == event_id)
        .expect("diagnostics must contain the truncated event");
    assert!(matches!(
        truncated_diag.decision,
        db::BackfillDecision::Skipped
    ));
    assert_eq!(
        truncated_diag.reason.as_deref(),
        Some(db::SKIPPED_TRUNCATED_REASON)
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn diagnostics_contain_skipped_event_ids() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let no_raw_event = db::record_request_record(
        &schema.pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/messages")
            .with_state(UsageEventKind::Request, RequestRecordState::Completed)
            .with_model(Some("public-model".to_string()))
            .with_timing(Some(200), Some(true), Some(10), Some(1))
            .with_usage(Some(100), Some(20), Some(120), None, None, None),
    )
    .await?;
    let truncated_event = insert_completed_with_tokens(
        &schema.pool,
        "/v1/messages",
        100,
        20,
        None,
        None,
        true,
        Some(ANTHROPIC_SSE_RESPONSE),
    )
    .await?;
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    let diag_ids: std::collections::HashSet<i64> =
        outcome.diagnostics.iter().map(|d| d.event_id).collect();
    assert!(
        diag_ids.contains(&no_raw_event),
        "missing-raw event must appear in diagnostics"
    );
    assert!(
        diag_ids.contains(&truncated_event),
        "truncated event must appear in diagnostics"
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn cursor_advances_past_skipped_and_failed_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping backfill test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = migrated_schema().await?;
    let first = db::record_request_record(
        &schema.pool,
        RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/messages")
            .with_state(UsageEventKind::Request, RequestRecordState::Completed)
            .with_model(Some("public-model".to_string()))
            .with_timing(Some(200), Some(true), Some(10), Some(1))
            .with_usage(Some(100), Some(20), Some(120), None, None, None),
    )
    .await?;
    let second = insert_old_anthropic(&schema.pool, "/v1/messages", ANTHROPIC_SSE_RESPONSE).await?;
    assert!(first < second);
    let outcome = run_batch(
        &schema.pool,
        BackfillOptions {
            apply: true,
            limit: 50,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(outcome.last_event_id, second);
    assert!(outcome.stats.scanned >= 2);
    schema.cleanup().await?;
    Ok(())
}
