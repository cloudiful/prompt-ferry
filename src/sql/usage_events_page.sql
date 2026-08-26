SELECT
    rr.event_id AS record_id,
    rr.request_id,
    rr.request_category,
    rr.user_id,
    u.login_name AS user_login_name,
    rr.client_key_label,
    rr.endpoint_id,
    pe.name AS endpoint_name,
    rr.mcp_server_id,
    COALESCE(rr.mcp_server_name, ms.name) AS mcp_server_name,
    rr.mcp_protocol_method,
    rr.mcp_operation_name,
    rr.path,
    rr.model,
    rr.request_state,
    rr.status,
    rr.ok,
    rr.duration_ms,
    rr.ttft_ms,
    rr.input_tokens,
    rr.output_tokens,
    rr.total_tokens,
    rr.cached_tokens,
    rr.cache_read_tokens,
    rr.cache_write_tokens,
    CASE
        WHEN GREATEST(
            COALESCE(rr.input_tokens, 0),
            GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
            0
        ) > 0
        THEN LEAST(
            1.0::DOUBLE PRECISION,
            GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)::DOUBLE PRECISION
            / GREATEST(
                COALESCE(rr.input_tokens, 0),
                GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                0
            )::DOUBLE PRECISION
        )
        ELSE NULL
    END AS cache_rate,
    rr.conversation_id,
    rr.parent_event_id,
    rr.conversation_seq,
    rr.conversation_source,
    rr.storage_sanitized,
    rr.storage_sanitized_nul_count,
    rr.redaction_applied AS applied,
    rr.redaction_findings_count AS findings_count,
    rr.redaction_replacements_count AS replacements_count,
    COALESCE(ARRAY(
        SELECT jsonb_array_elements_text(COALESCE(rr.redaction_types_json, '[]'::jsonb))
    ), ARRAY[]::TEXT[]) AS types,
    COALESCE(ARRAY(
        SELECT jsonb_array_elements_text(COALESCE(rr.redaction_fields_json, '[]'::jsonb))
    ), ARRAY[]::TEXT[]) AS fields,
    (rr.request_full_json IS NOT NULL OR rr.request_delta_json IS NOT NULL) AS has_full_request,
    (rr.parent_event_id IS NOT NULL) AS has_parent,
    rr.error_code,
    rr.error_message,
    rr.abort_reason,
    rr.abort_from_state,
    rr.abort_response_started,
    rr.failure_family,
    rr.mcp_bearer_token_slot,
    rr.route_selection_reason,
    rr.created_at
FROM request_records rr
LEFT JOIN users u ON u.user_id = rr.user_id
LEFT JOIN provider_endpoints pe ON pe.endpoint_id = rr.endpoint_id
LEFT JOIN mcp_servers ms ON ms.server_id = rr.mcp_server_id
WHERE rr.event_kind = 'request'
AND rr.request_category = $2
AND ($1::BIGINT IS NULL OR rr.user_id = $1)
AND ($3::TEXT IS NULL OR (
    rr.request_id::TEXT ILIKE '%' || $3 || '%'
    OR COALESCE(rr.conversation_id::TEXT, '') ILIKE '%' || $3 || '%'
    OR COALESCE(u.login_name, '') ILIKE '%' || $3 || '%'
    OR COALESCE(pe.name, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.mcp_server_name, ms.name, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.model, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.mcp_protocol_method, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.mcp_operation_name, '') ILIKE '%' || $3 || '%'
    OR rr.path ILIKE '%' || $3 || '%'
    OR COALESCE(rr.error_code, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.error_message, '') ILIKE '%' || $3 || '%'
    OR COALESCE(rr.client_key_label, '') ILIKE '%' || $3 || '%'
))
AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at >= $4)
AND ($5::TIMESTAMPTZ IS NULL OR rr.created_at < $5)
AND ($6::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $6)
AND (
    $7::TEXT IS NULL
    OR (
        rr.request_category = 'mcp'
        AND COALESCE(rr.mcp_server_name, ms.name, '-') = $7
    )
    OR (
        rr.request_category <> 'mcp'
        AND COALESCE(rr.model, '-') = $7
    )
)
AND ($8::UUID IS NULL OR rr.endpoint_id = $8)
AND ($9::UUID IS NULL OR rr.mcp_server_id = $9)
AND ($10::SMALLINT IS NULL OR rr.mcp_bearer_token_slot = $10)
AND ($11::TEXT IS NULL OR rr.request_state = $11)
AND ($12::BOOLEAN IS NULL OR rr.redaction_applied = $12)
AND ($13::BIGINT IS NULL OR rr.client_key_id = $13)
ORDER BY /*__ORDER_BY__*/
LIMIT $14 OFFSET $15
