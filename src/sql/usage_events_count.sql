SELECT COUNT(*)::BIGINT AS "total!"
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
