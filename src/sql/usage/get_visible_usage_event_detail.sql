SELECT
    ue.event_id AS "record_id!",
    ue.request_id AS "request_id!",
    ue.request_category AS "request_category!: _",
    ue.user_id,
    u.login_name AS user_login_name,
    ue.client_key_label,
    ue.request_user_agent,
    ue.http_request_content_encoding,
    ue.http_request_compressed AS "http_request_compressed!",
    ue.http_request_compressed_bytes,
    ue.http_request_decompressed_bytes,
    ue.http_request_compression_ratio,
    ue.endpoint_id,
    pe.name AS endpoint_name,
    ue.endpoint_key_id,
    ue.endpoint_key_label,
    ue.mcp_server_id,
    COALESCE(ue.mcp_server_name, ms.name) AS mcp_server_name,
    ue.mcp_protocol_method,
    ue.mcp_operation_name,
    ue.path AS "path!",
    ue.model,
    ue.request_state AS "request_state!: _",
    ue.status,
    ue.ok,
    ue.duration_ms,
    ue.ttft_ms,
    ue.input_tokens,
    ue.output_tokens,
    ue.total_tokens,
    ue.cached_tokens,
    ue.cache_read_tokens,
    ue.cache_write_tokens,
    CASE
        WHEN GREATEST(
            COALESCE(ue.input_tokens, 0),
            GREATEST(COALESCE(ue.cache_read_tokens, ue.cached_tokens, 0), 0)
                + GREATEST(COALESCE(ue.cache_write_tokens, 0), 0),
            0
        ) > 0
        THEN LEAST(
            1.0::DOUBLE PRECISION,
            GREATEST(COALESCE(ue.cache_read_tokens, ue.cached_tokens, 0), 0)::DOUBLE PRECISION
            / GREATEST(
                COALESCE(ue.input_tokens, 0),
                GREATEST(COALESCE(ue.cache_read_tokens, ue.cached_tokens, 0), 0)
                    + GREATEST(COALESCE(ue.cache_write_tokens, 0), 0),
                0
            )::DOUBLE PRECISION
        )
        ELSE NULL
    END AS cache_rate,
    ue.conversation_id,
    ue.parent_event_id,
    ue.conversation_seq,
    ue.conversation_source AS "conversation_source!",
    ue.storage_sanitized AS "storage_sanitized!",
    ue.storage_sanitized_nul_count AS "storage_sanitized_nul_count!",
    ue.redaction_applied AS "applied!",
    ue.redaction_findings_count AS "findings_count!",
    ue.redaction_replacements_count AS "replacements_count!",
    COALESCE(ARRAY(
        SELECT jsonb_array_elements_text(COALESCE(ue.redaction_types_json, '[]'::jsonb))
    ), ARRAY[]::TEXT[]) AS "types!",
    COALESCE(ARRAY(
        SELECT jsonb_array_elements_text(COALESCE(ue.redaction_fields_json, '[]'::jsonb))
    ), ARRAY[]::TEXT[]) AS "fields!",
    ue.client_installation_id,
    ue.normalized_item_count,
    COALESCE(ue.request_storage_mode, 'full') AS "request_storage_mode!",
    raw.request_raw_json,
    raw.raw_object_key,
    raw.raw_object_sha256,
    raw.raw_object_expires_at,
    ue.request_has_previous_response_id AS "request_has_previous_response_id!",
    ue.request_previous_response_id,
    ue.request_previous_response_parent_found,
    ue.request_conversation_key,
    ue.request_conversation_parent_found,
    ue.provider_response_id,
    (ue.request_full_json IS NOT NULL OR ue.request_delta_json IS NOT NULL) AS "has_full_request!",
    (ue.parent_event_id IS NOT NULL) AS "has_parent!",
    ue.response_prompt,
    raw.response_raw_body,
    ua.message_json -> 'assistant_message' AS assistant_message_json,
    ua.message_json -> 'output_items' AS assistant_output_items_json,
    ua.has_reasoning_content,
    ue.upstream_error_body,
    ue.error_code,
    ue.error_message,
    ue.abort_reason AS "abort_reason: _",
    ue.abort_from_state AS "abort_from_state: _",
    ue.abort_response_started,
    ue.failure_family AS "failure_family: _",
    ue.mcp_bearer_token_slot,
    ue.route_selection_reason AS "route_selection_reason!: _",
    ue.created_at AS "created_at!",
    ue.response_capture_truncated AS "response_capture_truncated!"
FROM request_records ue
LEFT JOIN users u ON u.user_id = ue.user_id
LEFT JOIN provider_endpoints pe ON pe.endpoint_id = ue.endpoint_id
LEFT JOIN mcp_servers ms ON ms.server_id = ue.mcp_server_id
LEFT JOIN request_record_assistant_artifacts ua ON ua.event_id = ue.event_id
LEFT JOIN request_record_raw_payloads raw
  ON raw.event_id = ue.event_id
 AND raw.created_at = ue.created_at
WHERE ue.event_id = $1
  AND ($2::BIGINT IS NULL OR ue.user_id = $2)
