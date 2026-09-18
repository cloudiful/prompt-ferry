-- P0 (issue #277): the facet dropdowns used to GROUP BY every row of
-- `request_records` with no bound, which scanned 569k rows (2.27s). Bound the
-- scan to a recent window ($3, defaulting to 30d) with an optional upper bound
-- ($4) and cap each facet branch ($5); together with
-- `idx_request_records_usage_covering` this keeps the branches index-only.
-- Issue #467: window-following branches converge on the caller-selected
-- [start, end) window; the `date` branch stays anchored on now() (calendar
-- dimension, independent of the user window). New `state`/`redaction`
-- branches feed the status/redaction dropdowns.
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
          AND rr.created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4::TIMESTAMPTZ)
        GROUP BY COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-')
        ORDER BY "value!" DESC
        LIMIT $5
    )
    UNION ALL
    (
        SELECT 'model' AS "facet!", COALESCE(model, '-') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = 'ai'
          AND $2 = 'ai'
          AND created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR created_at < $4::TIMESTAMPTZ)
        GROUP BY COALESCE(model, '-')
        ORDER BY "value!" DESC
        LIMIT $5
    )
    UNION ALL
    (
        SELECT 'target' AS "facet!", COALESCE(mcp_server_name, '-') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = 'mcp'
          AND $2 = 'mcp'
          AND created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR created_at < $4::TIMESTAMPTZ)
        GROUP BY COALESCE(mcp_server_name, '-')
        ORDER BY "value!" DESC
        LIMIT $5
    )
    UNION ALL
    (
        SELECT 'date' AS "facet!", to_char(created_at, 'YYYY-MM-DD') AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records
        WHERE event_kind = 'request'
          AND ($1::BIGINT IS NULL OR user_id = $1)
          AND request_category = $2
          AND created_at >= now() - interval '30 days'
          AND created_at < now()
        GROUP BY to_char(created_at, 'YYYY-MM-DD')
        ORDER BY "value!" DESC
        LIMIT $5
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
          AND rr.created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4::TIMESTAMPTZ)
        GROUP BY rr.client_key_id, COALESCE(rr.client_key_label, '-'), u.login_name
        ORDER BY "value!" DESC
        LIMIT $5
    )
    UNION ALL
    (
        SELECT 'state' AS "facet!", rr.request_state AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records rr
        WHERE rr.event_kind = 'request'
          AND ($1::BIGINT IS NULL OR rr.user_id = $1)
          AND rr.request_category = $2
          AND rr.created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4::TIMESTAMPTZ)
        GROUP BY rr.request_state
        ORDER BY "value!" DESC
        LIMIT $5
    )
    UNION ALL
    (
        SELECT 'redaction' AS "facet!", rr.redaction_applied::TEXT AS "value!", NULL::BIGINT AS "key_id?", NULL::TEXT AS "label?", NULL::TEXT AS "user_login_name?"
        FROM request_records rr
        WHERE rr.event_kind = 'request'
          AND ($1::BIGINT IS NULL OR rr.user_id = $1)
          AND rr.request_category = $2
          AND rr.created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '30 days')
          AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4::TIMESTAMPTZ)
        GROUP BY rr.redaction_applied
        ORDER BY "value!" DESC
        LIMIT $5
    )
) facets
ORDER BY "facet!" ASC, "value!" DESC
