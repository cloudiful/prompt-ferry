use super::*;
use crate::raw_payload_store::{RawPayloadEnvelope, RawPayloadStore};
use chrono::Duration as ChronoDuration;
use tracing::warn;

pub async fn record_request_record(pool: &PgPool, input: RequestRecordCreate) -> Result<i64> {
    let ref_full = input.request_full_json.clone();
    let ref_delta = input.request_delta_json.clone();
    let mut tx = pool.begin().await?;
    let event_id = sqlx::query_file_scalar!(
        "src/sql/usage/upsert_request_record.sql",
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
        input.request_full_json,
        input.request_delta_json,
        input.request_has_previous_response_id,
        input.request_previous_response_id,
        input.request_previous_response_parent_found,
        input.request_conversation_key,
        input.request_conversation_parent_found,
        input.upstream_redaction_enabled,
        input.provider_response_id,
        input.provider_conversation_key,
        input.base_checkpoint_event_id,
        input.response_prompt,
        input.upstream_error_body,
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
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    if let Err(err) = crate::db::record_usage_charge(pool, event_id, &input).await {
        warn!(
            error = %err,
            event_id,
            input_tokens = input.input_tokens,
            cache_read_tokens = input.cache_read_tokens,
            cache_write_tokens = input.cache_write_tokens,
            output_tokens = input.output_tokens,
            "failed to persist usage billing snapshot"
        );
    }

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

    insert_request_record_block_refs(pool, event_id, &ref_full, &ref_delta).await?;

    Ok(event_id)
}

/// Persist the request-record metadata first, then upload the raw payload to
/// the selected store and record its object metadata. There is deliberately no
/// PostgreSQL body fallback: when the store upload fails the raw payload is
/// dropped with a warning while the metadata event survives.
pub async fn record_request_record_with_raw_store(
    pool: &PgPool,
    input: RequestRecordCreate,
    raw_store: Option<&RawPayloadStore>,
    raw_retention_days: i64,
) -> Result<i64> {
    let payload = RawPayloadEnvelope {
        request_raw_json: input.request_raw_json.clone(),
        response_raw_body: input.response_raw_body.clone(),
    };
    let event_id = record_request_record(pool, input).await?;
    let Some(raw_store) = raw_store else {
        return Ok(event_id);
    };
    if payload.request_raw_json.is_none() && payload.response_raw_body.is_none() {
        return Ok(event_id);
    }

    let created_at =
        sqlx::query_file_scalar!("src/sql/usage/get_request_record_created_at.sql", event_id,)
            .fetch_optional(pool)
            .await?;
    let Some(created_at) = created_at else {
        return Ok(event_id);
    };
    let expires_at = created_at + ChronoDuration::days(raw_retention_days.max(1));
    // The store merges this phase's fields into any existing per-event object,
    // so a request-only write followed by a response-only write is idempotent.
    let object = match raw_store
        .put(event_id, created_at, payload, expires_at)
        .await
    {
        Ok(object) => object,
        Err(err) => {
            warn!(error = %err, event_id, "failed to upload raw payload; dropping raw payload");
            return Ok(event_id);
        }
    };
    if let Err(err) = sqlx::query_file!(
        "src/sql/usage/upsert_request_record_raw_object.sql",
        event_id,
        object.object_key,
        object.size_bytes,
        object.sha256,
        object.expires_at,
    )
    .execute(pool)
    .await
    {
        warn!(
            error = %err,
            event_id,
            "failed to persist raw payload object metadata"
        );
    }
    Ok(event_id)
}

async fn insert_request_record_block_refs(
    pool: &PgPool,
    event_id: i64,
    request_full_json: &Option<Value>,
    request_delta_json: &Option<Value>,
) -> Result<()> {
    for json_ref in [request_full_json, request_delta_json] {
        let Some(json) = json_ref else { continue };
        if json.is_null() || json.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            continue;
        }
        sqlx::query_file!(
            "src/sql/usage/insert_request_record_block_refs.sql",
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
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    fn max_placeholder(sql: &str) -> usize {
        let mut max = 0usize;
        let bytes = sql.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if bytes[i] == b'$' {
                let mut j = i + 1;
                let mut value = 0usize;
                let mut found = false;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    found = true;
                    value = value * 10 + (bytes[j] - b'0') as usize;
                    j += 1;
                }
                if found {
                    max = max.max(value);
                    i = j;
                    continue;
                }
            }
            i += 1;
        }

        max
    }

    fn insert_column_count(sql: &str) -> usize {
        let (_, rest) = sql.split_once('(').expect("sql has insert columns");
        let (cols, _) = rest.split_once(")\nVALUES").expect("sql has values clause");
        cols.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .count()
    }

    #[test]
    fn request_record_sql_placeholders_match_bind_count() {
        let upsert_sql = include_str!("../../sql/usage/upsert_request_record.sql");

        assert_eq!(insert_column_count(upsert_sql), 74);
        assert_eq!(max_placeholder(upsert_sql), 74);
    }
}
