use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{RequestRecordToolCall, RequestRecordToolCallCreate, RequestToolCallStatus};

fn parse_request_tool_call_status(value: &str) -> Result<RequestToolCallStatus> {
    match value {
        "emitted" => Ok(RequestToolCallStatus::Emitted),
        "output_received" => Ok(RequestToolCallStatus::OutputReceived),
        "failed" => Ok(RequestToolCallStatus::Failed),
        "skipped" => Ok(RequestToolCallStatus::Skipped),
        other => Err(anyhow!("unknown request tool call status `{other}`")),
    }
}

pub async fn allocate_conversation_seq(
    pool: &PgPool,
    conversation_id: Uuid,
    minimum_seq: i32,
) -> Result<i32> {
    Ok(sqlx::query_file_scalar!(
        "src/sql/usage/allocate_conversation_seq.sql",
        conversation_id,
        minimum_seq,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn heartbeat_request_record_lease(
    pool: &PgPool,
    request_id: Uuid,
    owner_worker_id: Option<Uuid>,
    lease_expires_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
) -> Result<u64> {
    let result = sqlx::query_file!(
        "src/sql/usage/heartbeat_request_record_lease.sql",
        request_id,
        owner_worker_id,
        lease_expires_at,
        last_heartbeat_at,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete_request_record_lease(pool: &PgPool, request_id: Uuid) -> Result<u64> {
    let result = sqlx::query_file!("src/sql/usage/delete_request_record_lease.sql", request_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn abort_stale_request_records(pool: &PgPool) -> Result<u64> {
    let result = sqlx::query_file!("src/sql/usage/abort_stale_request_records.sql")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn list_active_request_record_ids(pool: &PgPool) -> Result<Vec<Uuid>> {
    let rows = sqlx::query_file_scalar!("src/sql/usage/list_active_request_record_ids.sql")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn abort_request_records_by_ids(pool: &PgPool, request_ids: &[Uuid]) -> Result<u64> {
    if request_ids.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query_file!(
        "src/sql/usage/abort_request_records_by_ids.sql",
        request_ids
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn upsert_request_record_tool_call(
    pool: &PgPool,
    input: RequestRecordToolCallCreate,
) -> Result<RequestRecordToolCall> {
    let row = sqlx::query_file!(
        "src/sql/usage/upsert_request_record_tool_call.sql",
        input.parent_event_id,
        input.conversation_id,
        input.call_id,
        input.tool_name,
        input.arguments_json,
        input.arguments_preview,
        input.status as _,
        input.sequence_in_turn,
        input.mcp_request_event_id
    )
    .fetch_one(pool)
    .await?;
    Ok(RequestRecordToolCall {
        tool_call_event_id: row.tool_call_event_id,
        parent_event_id: row.parent_event_id,
        conversation_id: row.conversation_id,
        call_id: row.call_id,
        tool_name: row.tool_name,
        arguments_json: row.arguments_json,
        arguments_preview: row.arguments_preview,
        status: parse_request_tool_call_status(&row.status)?,
        sequence_in_turn: row.sequence_in_turn,
        mcp_request_event_id: row.mcp_request_event_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub async fn list_request_record_tool_calls(
    pool: &PgPool,
    parent_event_id: i64,
) -> Result<Vec<RequestRecordToolCall>> {
    let rows = sqlx::query_file!(
        "src/sql/usage/list_request_record_tool_calls.sql",
        parent_event_id
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(RequestRecordToolCall {
                tool_call_event_id: row.tool_call_event_id,
                parent_event_id: row.parent_event_id,
                conversation_id: row.conversation_id,
                call_id: row.call_id,
                tool_name: row.tool_name,
                arguments_json: row.arguments_json,
                arguments_preview: row.arguments_preview,
                status: parse_request_tool_call_status(&row.status)?,
                sequence_in_turn: row.sequence_in_turn,
                mcp_request_event_id: row.mcp_request_event_id,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

pub async fn find_request_record_tool_calls_by_call_ids(
    pool: &PgPool,
    call_ids: &[String],
    user_id: Option<i64>,
    endpoint_id: Option<Uuid>,
) -> Result<Vec<RequestRecordToolCall>> {
    if call_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_file!(
        "src/sql/usage/find_request_record_tool_calls_by_call_ids.sql",
        call_ids,
        user_id,
        endpoint_id,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(RequestRecordToolCall {
                tool_call_event_id: row.tool_call_event_id,
                parent_event_id: row.parent_event_id,
                conversation_id: row.conversation_id,
                call_id: row.call_id,
                tool_name: row.tool_name,
                arguments_json: row.arguments_json,
                arguments_preview: row.arguments_preview,
                status: parse_request_tool_call_status(&row.status)?,
                sequence_in_turn: row.sequence_in_turn,
                mcp_request_event_id: row.mcp_request_event_id,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .collect()
}
