use super::*;
use crate::db::seaorm::PostgresNativeConnection;
use sea_orm::ConnectionTrait;
use sea_orm::sea_query::{Alias, Expr, Query};
use tracing::warn;

/// Persist the request record, then optionally store the captured raw request
/// and response bodies next to it.
///
/// The metadata and normalized content are written in one transaction first;
/// the bodies are a post-commit upsert so a body failure can never roll back
/// the request event. When both bodies are absent, or `raw_retention_days` is
/// zero, no body row is written at all.
pub async fn record_request_record(
    pool: &PgPool,
    input: RequestRecordCreate,
    raw_retention_days: i64,
) -> Result<i64> {
    let request_raw_json = input.request_raw_json.clone();
    let response_raw_body = input.response_raw_body.clone();
    let (event_id, created_at) = record_request_record_with_created_at(pool, input).await?;

    if raw_retention_days > 0
        && (request_raw_json.is_some() || response_raw_body.is_some())
        && let Err(err) = sqlx::query_file!(
            "src/sql/usage/upsert_request_record_raw_bodies.sql",
            event_id,
            created_at,
            request_raw_json,
            response_raw_body,
        )
        .execute(pool)
        .await
    {
        warn!(
            error = %err,
            event_id,
            has_request_body = request_raw_json.is_some(),
            has_response_body = response_raw_body.is_some(),
            "failed to persist raw request bodies; keeping the request record"
        );
    }

    Ok(event_id)
}

/// Persist the request record and return its canonical identity: the
/// `event_id` and the `created_at` partition key the row was written with.
/// The caller reuses that timestamp instead of looking the row up again.
async fn record_request_record_with_created_at(
    pool: &PgPool,
    input: RequestRecordCreate,
) -> Result<(i64, DateTime<Utc>)> {
    let ref_full = input.request_full_json.clone();
    let ref_delta = input.request_delta_json.clone();
    let node_attribution = input.node_attribution;
    let mut tx = pool.begin().await?;
    let record = sqlx::query_file!(
        "src/sql/usage/upsert_request_record.sql",
        input.created_at,
        input.event_kind.as_str(),
        input.request_category.as_str(),
        input.request_state.as_str(),
        input.request_id,
        input.user_id,
        input.client_key_label,
        input.request_user_agent,
        input.endpoint_id,
        input.endpoint_key_id,
        input.endpoint_key_label,
        input.model_route_rule_id,
        input.mcp_server_id,
        input.mcp_server_name,
        input.mcp_protocol_method,
        input.mcp_operation_name,
        input.path,
        input.http_request_content_encoding,
        input.http_request_compressed,
        input.http_request_compressed_bytes,
        input.http_request_decompressed_bytes,
        input.http_request_compression_ratio,
        input.model,
        input.status,
        input.ok,
        input.duration_ms,
        input.ttft_ms,
        input.input_tokens,
        input.output_tokens,
        input.total_tokens,
        input.cached_tokens,
        input.cache_read_tokens,
        input.cache_write_tokens,
        input.conversation_id,
        input.parent_event_id,
        input.conversation_seq,
        input.conversation_source,
        input.storage_sanitized,
        input.storage_sanitized_nul_count,
        input.redaction_applied,
        input.redaction_findings_count,
        input.redaction_replacements_count,
        input.redaction_types_json,
        input.redaction_fields_json,
        input.client_installation_id,
        input.normalized_item_count,
        input.normalized_chain_hash,
        input.normalized_first_ref_hash,
        input.normalized_last_ref_hash,
        input.request_storage_mode,
        input.request_has_previous_response_id,
        input.request_previous_response_id,
        input.request_previous_response_parent_found,
        input.request_conversation_key,
        input.request_conversation_parent_found,
        input.upstream_redaction_enabled,
        input.provider_response_id,
        input.provider_conversation_key,
        input.base_checkpoint_event_id,
        input.error_code,
        input.error_message,
        input.failure_family.map(|value| value.as_str()),
        input.mcp_bearer_token_slot,
        input.route_selection_reason.as_str(),
        input.owner_worker_id,
        input.lease_expires_at,
        input.last_heartbeat_at,
        input.response_capture_truncated,
        input.client_key_id,
        input.requested_model,
        input.upstream_model,
        input.applied_thinking_effort_override.as_deref(),
        input.session_header_id,
        input.session_parent_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    let event_id = record.event_id;
    let created_at = record.created_at;

    // Issue #277 Phase P7: the content half lives in its own daily partition.
    // A row is written even when every payload column is NULL, because a
    // missing row is what "expired" means after the split.
    sqlx::query_file!(
        "src/sql/usage/upsert_request_record_content.sql",
        input.created_at,
        event_id,
        ref_full.as_deref(),
        ref_delta.as_ref(),
        input.response_prompt,
        input.upstream_error_body,
    )
    .execute(&mut *tx)
    .await?;

    // The node ownership of the record is written in the same transaction as
    // the row itself: a failed attribution write rolls the record back. The
    // update is null-only, so the lifecycle upserts of the same request never
    // overwrite the ownership captured by the first write, and its `created_at`
    // predicate prunes the statement to the row's daily partition.
    if let Some(node_attribution) = node_attribution {
        use sea_orm::sea_query::ExprTrait;
        let document = serde_json::to_value(node_attribution)?;
        let statement = Query::update()
            .table(Alias::new("request_records"))
            .value(Alias::new("node_attribution"), Expr::value(document))
            .and_where(Expr::col(Alias::new("event_id")).eq(event_id))
            .and_where(Expr::col(Alias::new("created_at")).eq(created_at))
            .and_where(Expr::col(Alias::new("node_attribution")).is_null())
            .to_owned();
        PostgresNativeConnection::new(&mut tx)
            .execute(&statement)
            .await
            .map_err(crate::db::seaorm::native_error)?;
    }

    tx.commit().await?;

    if input.request_state.is_terminal() {
        delete_request_record_lease(pool, input.request_id).await?;
    } else if let (Some(lease_expires_at), Some(last_heartbeat_at)) =
        (input.lease_expires_at, input.last_heartbeat_at)
    {
        let _ = heartbeat_request_record_lease(
            pool,
            input.request_id,
            input.owner_worker_id,
            lease_expires_at,
            last_heartbeat_at,
        )
        .await?;
    }

    insert_request_record_block_refs(
        pool,
        event_id,
        input.created_at,
        ref_full.as_deref(),
        ref_delta.as_ref(),
    )
    .await?;

    Ok((event_id, created_at))
}

async fn insert_request_record_block_refs(
    pool: &PgPool,
    event_id: i64,
    created_at: DateTime<Utc>,
    request_full_json: Option<&Value>,
    request_delta_json: Option<&Value>,
) -> Result<()> {
    for json in [request_full_json, request_delta_json]
        .into_iter()
        .flatten()
    {
        if json.is_null() || json.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            continue;
        }
        sqlx::query_file!(
            "src/sql/usage/insert_request_record_block_refs.sql",
            created_at,
            event_id,
            json,
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub struct RequestRecordStateInput<'a> {
    pub request_id: uuid::Uuid,
    pub request_state: RequestRecordState,
    /// Issue #277 Phase P7: pins the update to the row's partition when known.
    pub created_at: Option<DateTime<Utc>>,
    pub endpoint_id: Option<uuid::Uuid>,
    pub model_route_rule_id: Option<uuid::Uuid>,
    pub model: Option<&'a str>,
    pub endpoint_key_id: Option<uuid::Uuid>,
    pub endpoint_key_label: Option<&'a str>,
}

pub async fn record_request_state(pool: &PgPool, input: RequestRecordStateInput<'_>) -> Result<()> {
    sqlx::query_file!(
        "src/sql/usage/update_request_record_state.sql",
        input.request_state.as_str(),
        input.endpoint_id,
        input.model_route_rule_id,
        input.model,
        input.endpoint_key_id,
        input.endpoint_key_label,
        input.request_id,
        input.created_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Issue #657 Phase P1: stamp the first meaningful output instant on the
/// running request row. The update keeps an already recorded `ttft_ms` and
/// leaves every other column (state included) untouched, so a repeated
/// observation, an upstream retry, or a write racing the terminal record stays
/// idempotent. Returns the affected row count; `created_at` pins the update to
/// the row's partition when known.
pub async fn record_request_first_output(
    pool: &PgPool,
    request_id: uuid::Uuid,
    created_at: Option<DateTime<Utc>>,
    ttft_ms: i64,
) -> Result<u64> {
    let result = sqlx::query_file!(
        "src/sql/usage/update_request_record_first_output.sql",
        ttft_ms,
        request_id,
        created_at,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
