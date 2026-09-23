//! Shared fixtures for the Phase P8 admin clear regression tests.

#![allow(dead_code)] // each clear test target uses a subset of these fixtures.

use chrono::{DateTime, Utc};
use prompt_ferry::db;
use serde_json::json;
use uuid::Uuid;

pub async fn create_record(pool: &sqlx::PgPool, user_id: Option<i64>) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(user_id, None, None, None),
    )
    .await
}

pub async fn create_user(pool: &sqlx::PgPool, login: &str) -> anyhow::Result<i64> {
    Ok(db::create_user(
        pool,
        db::UserCreate {
            login_name: login.to_string(),
            password_hash: "unused".to_string(),
            display_name: login.to_string(),
            is_admin: false,
        },
    )
    .await?
    .user_id)
}

/// A completed request plus one row in every content-family table, all on the
/// same day so the family shares one partition.
pub async fn create_today_record_with_family(
    pool: &sqlx::PgPool,
    user_id: Option<i64>,
) -> anyhow::Result<(i64, DateTime<Utc>, String)> {
    let block_hash = format!("p8-block-{}", Uuid::new_v4());
    let mut record = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        )
        .with_request_actor(user_id, None, None, None);
    record.request_full_json = Some(Box::new(json!([
        {"role": "user", "block_hash": block_hash}
    ])));
    let created_at = record.created_at;
    let event_id = db::record_request_record(pool, record).await?;

    db::upsert_usage_prompt_block(
        pool,
        created_at,
        &block_hash,
        "user",
        &json!({"role": "user", "content": "family"}),
        "family",
    )
    .await?;
    db::upsert_usage_assistant_artifact(
        pool,
        db::UsageAssistantArtifactCreate {
            event_id,
            created_at,
            message_json: json!({"role": "assistant"}),
            has_reasoning_content: false,
            has_tool_calls: false,
        },
    )
    .await?;
    db::upsert_request_record_tool_call(
        pool,
        db::RequestRecordToolCallCreate {
            created_at,
            parent_event_id: event_id,
            conversation_id: None,
            call_id: "call_1".to_string(),
            tool_name: "lookup".to_string(),
            arguments_json: Some(json!({})),
            arguments_preview: Some("{}".to_string()),
            status: db::RequestToolCallStatus::Emitted,
            sequence_in_turn: Some(0),
            mcp_request_event_id: None,
        },
    )
    .await?;
    db::insert_replay_snapshot(
        pool,
        db::ReplaySnapshotCreate {
            event_id,
            created_at,
            conversation_id: Uuid::new_v4(),
            conversation_seq: 1,
            base_event_id: event_id,
            prompt_refs_json: json!([]),
            ref_count: 0,
            byte_size: 0,
        },
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_request_record_raw_payload.sql",
        event_id,
        created_at,
    )
    .execute(pool)
    .await?;

    Ok((event_id, created_at, block_hash))
}

pub async fn count_partition(pool: &sqlx::PgPool, partition: &str) -> anyhow::Result<i64> {
    let row = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_partition_by_name.sql",
        partition,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.count)
}

pub async fn count_metadata(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<i64> {
    let row = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.count)
}

/// Assert that every content-family table is empty for one event and that its
/// prompt block is gone.
pub async fn assert_family_rows_absent(
    pool: &sqlx::PgPool,
    event_id: i64,
    block_hash: &str,
) -> anyhow::Result<()> {
    let content = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_content.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let block_refs = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_block_refs.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let artifacts = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_artifacts.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let tool_calls = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_tool_calls.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let snapshots = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_snapshots.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let raw_payloads = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_raw_payloads.sql",
        event_id,
    )
    .fetch_one(pool)
    .await?
    .count;
    let prompt_block = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_usage_prompt_blocks.sql",
        block_hash,
    )
    .fetch_one(pool)
    .await?
    .count;

    for (label, count) in [
        ("content", content),
        ("block refs", block_refs),
        ("artifacts", artifacts),
        ("tool calls", tool_calls),
        ("snapshots", snapshots),
        ("raw payloads", raw_payloads),
        ("prompt block", prompt_block),
    ] {
        assert_eq!(count, 0, "{label} must be gone after the clear");
    }
    Ok(())
}

pub fn clear_query(
    scope: db::UsageClearScope,
    visible_user_id: Option<i64>,
    target_user_id: Option<i64>,
    start_at: Option<DateTime<Utc>>,
    end_at: Option<DateTime<Utc>>,
) -> db::UsageClearQuery {
    db::UsageClearQuery {
        scope,
        visible_user_id,
        target_user_id,
        start_at,
        end_at,
    }
}
