-- P0 (issue #277): the facet dropdowns used to GROUP BY every row of
-- `request_records` with no bound, which scanned 569k rows (2.27s). Bound the
-- scan to a recent window ($3) and cap each facet branch ($4); together with
-- `idx_request_records_usage_covering` this keeps the branches index-only.
-- Branch-level ORDER BY/LIMIT requires the parentheses around each SELECT.
SELECT "facet!" AS "facet!", "value!" AS "value!", "key_id?" AS "key_id?", "label?" AS "label?", "user_login_name?" AS "user_login_name?"
FROM (
    (
        SELECT 'user' AS "facet!", COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records rr
        LEFT JOIN users u ON u.user_id = rr.user_id
        WHERE rr.event_kind = 'request'
          AND ($1::BIGINT IS NULL OR rr.user_id = $1)
          AND rr.request_category = $2
          AND rr.created_at >= $3
        GROUP BY COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-')
        ORDER BY "value!" DESC
        LIMIT $4
    )
    UNION ALL
    (
        SELECT 'model' AS "facet!", COALESCE(model, '-') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = 'ai'
          AND $2 = 'ai'
          AND created_at >= $3
        GROUP BY COALESCE(model, '-')
        ORDER BY "value!" DESC
        LIMIT $4
    )
    UNION ALL
    (
        SELECT 'target' AS "facet!", COALESCE(mcp_server_name, '-') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = 'mcp'
          AND $2 = 'mcp'
          AND created_at >= $3
        GROUP BY COALESCE(mcp_server_name, '-')
        ORDER BY "value!" DESC
        LIMIT $4
    )
    UNION ALL
    (
        SELECT 'date' AS "facet!", to_char(created_at, 'YYYY-MM-DD') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = $2
          AND created_at >= $3
        GROUP BY to_char(created_at, 'YYYY-MM-DD')
        ORDER BY "value!" DESC
        LIMIT $4
    )
    UNION ALL
    (
        SELECT 'client_key' AS "facet!", COALESCE(rr.client_key_label, '-') AS "value!", rr.client_key_id AS "key_id?", COALESCE(rr.client_key_label, '-') AS "label?", u.login_name AS "user_login_name?"
        FROM request_records rr
        LEFT JOIN users u ON u.user_id = rr.user_id
        WHERE rr.event_kind = 'request'
          AND ($1::BIGINT IS NULL OR rr.user_id = $1)
          AND rr.request_category = $2
          AND rr.client_key_id IS NOT NULL
          AND rr.created_at >= $3
        GROUP BY rr.client_key_id, COALESCE(rr.client_key_label, '-'), u.login_name
        ORDER BY "value!" DESC
        LIMIT $4
    )
) facets
ORDER BY "facet!" ASC, "value!" DESC
