SELECT
    COALESCE(rr.mcp_server_name, ms.name, '(unknown)') AS label,
    NULL::TEXT AS secondary_label,
    rr.endpoint_id,
    rr.model,
    rr.mcp_server_id,
    rr.mcp_bearer_token_slot,
    COUNT(*)::BIGINT AS "request_count!",
    COUNT(*) FILTER (WHERE rr.ok IS TRUE)::BIGINT AS "success_count!",
    COUNT(*) FILTER (WHERE rr.failure_family = 'empty_success')::BIGINT AS "empty_success_count!",
    COUNT(*) FILTER (WHERE rr.failure_family = 'rate_limit' OR rr.failure_family = 'quota')::BIGINT AS "rate_limit_count!",
    COUNT(*) FILTER (WHERE rr.failure_family = 'auth')::BIGINT AS "auth_error_count!",
    COUNT(*) FILTER (WHERE rr.failure_family = 'upstream_5xx' OR rr.failure_family = 'network')::BIGINT AS "upstream_5xx_count!",
    COUNT(*) FILTER (WHERE COALESCE(rr.cached_tokens, 0) > 0 OR COALESCE(rr.cache_read_tokens, 0) > 0)::BIGINT AS "cache_hit_count!",
    COUNT(DISTINCT rr.mcp_protocol_method)::BIGINT AS "method_coverage_count!",
    percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms)
        FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms,
    percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.ttft_ms)
        FILTER (WHERE rr.ttft_ms IS NOT NULL) AS p95_first_token_ms
FROM request_records rr
LEFT JOIN users u ON u.user_id = rr.user_id
LEFT JOIN provider_endpoints pe ON pe.endpoint_id = rr.endpoint_id
LEFT JOIN mcp_servers ms ON ms.server_id = rr.mcp_server_id
WHERE rr.event_kind = 'request'
  AND rr.request_category = $2
  AND ($1::BIGINT IS NULL OR rr.user_id = $1)
  AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
  AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
  AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
GROUP BY rr.mcp_server_id, rr.mcp_server_name, ms.name, rr.endpoint_id, rr.model, rr.mcp_bearer_token_slot
ORDER BY 7 DESC, label ASC
LIMIT 12
