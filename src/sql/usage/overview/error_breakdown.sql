SELECT
    COALESCE(rr.failure_family, 'unknown') AS "key!",
    COUNT(*)::BIGINT AS "count!"
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
  AND rr.failure_family IS NOT NULL
GROUP BY 1
ORDER BY 2 DESC, 1 ASC
