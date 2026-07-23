use super::*;

pub async fn record_request_record(pool: &PgPool, input: RequestRecordCreate) -> Result<i64> {
    let ref_full = input.request_full_json.clone();
    let ref_delta = input.request_delta_json.clone();
    let raw_request_json = input.request_raw_json.clone();
    let raw_response_body = input.response_raw_body.clone();
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
        input.first_chunk_ms,
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
        input.upstream_redacted_request_json,
        input.restore_session.ciphertext,
        input.restore_session.nonce,
        input.restore_session.key_version,
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
    )
    .fetch_one(&mut *tx)
    .await?;

    if raw_request_json.is_some() || raw_response_body.is_some() {
        sqlx::query_file!(
            "src/sql/usage/upsert_request_record_raw_payload.sql",
            event_id,
            raw_request_json,
            raw_response_body,
        )
        .execute(&mut *tx)
        .await?;
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

    insert_request_record_block_refs(pool, event_id, &ref_full, &ref_delta).await?;

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

pub async fn record_request_state(
    pool: &PgPool,
    request_id: uuid::Uuid,
    request_state: RequestRecordState,
    endpoint_id: Option<uuid::Uuid>,
    model_route_rule_id: Option<uuid::Uuid>,
    model: Option<&str>,
    endpoint_key_id: Option<uuid::Uuid>,
    endpoint_key_label: Option<&str>,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/usage/update_request_record_state.sql",
        request_state.as_str(),
        endpoint_id,
        model_route_rule_id,
        model,
        endpoint_key_id,
        endpoint_key_label,
        request_id,
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
