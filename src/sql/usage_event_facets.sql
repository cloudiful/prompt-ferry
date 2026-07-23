SELECT "facet!" AS "facet!", "value!" AS "value!"
FROM (
    SELECT 'user' AS "facet!", COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') AS "value!"
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND rr.request_category = $2
    GROUP BY COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-')
    UNION ALL
    SELECT 'model' AS "facet!", COALESCE(model, '-') AS "value!"
    FROM request_records
    WHERE event_kind = 'request'
      AND ($1::BIGINT IS NULL OR user_id = $1)
      AND request_category = 'ai'
      AND $2 = 'ai'
    GROUP BY COALESCE(model, '-')
    UNION ALL
    SELECT 'target' AS "facet!", COALESCE(mcp_server_name, '-') AS "value!"
    FROM request_records
    WHERE event_kind = 'request'
      AND ($1::BIGINT IS NULL OR user_id = $1)
      AND request_category = 'mcp'
      AND $2 = 'mcp'
    GROUP BY COALESCE(mcp_server_name, '-')
    UNION ALL
    SELECT 'date' AS "facet!", to_char(created_at, 'YYYY-MM-DD') AS "value!"
    FROM request_records
    WHERE event_kind = 'request'
      AND ($1::BIGINT IS NULL OR user_id = $1)
      AND request_category = $2
    GROUP BY to_char(created_at, 'YYYY-MM-DD')
) facets
ORDER BY "facet!" ASC, "value!" DESC
