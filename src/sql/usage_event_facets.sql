-- Facet dropdown options (issue #277): a single scan over `request_records`
-- derives the user/model/target/date/client_key/state/redaction groups with
-- `GROUPING SETS`, then keeps the top `$5` values per facet via
-- `row_number() OVER (PARTITION BY facet ORDER BY value DESC)`. The `model`
-- facet only applies to the `ai` category, `target` only to `mcp`, and
-- `client_key` only to rows carrying a `client_key_id`. The scan follows the
-- caller-selected `[start, end)` window ($3/$4), falling back to the last 24
-- hours ending now, and stays aligned with `idx_request_records_usage_covering`.
WITH bounded AS (
    SELECT
        COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') AS user_value,
        CASE WHEN $2 = 'ai' THEN COALESCE(rr.model, '-') END AS model_value,
        CASE WHEN $2 = 'mcp' THEN COALESCE(rr.mcp_server_name, '-') END AS target_value,
        to_char(rr.created_at, 'YYYY-MM-DD') AS date_value,
        CASE WHEN rr.client_key_id IS NOT NULL THEN rr.client_key_id END AS client_key_id,
        CASE WHEN rr.client_key_id IS NOT NULL THEN COALESCE(rr.client_key_label, '-') END AS client_key_value,
        u.login_name AS user_login_name,
        rr.request_state AS request_state,
        rr.redaction_applied::TEXT AS redaction_value
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND rr.request_category = $2
      AND rr.created_at >= COALESCE($3::TIMESTAMPTZ, now() - interval '24 hours')
      AND rr.created_at < COALESCE($4::TIMESTAMPTZ, now())
)
SELECT
    facet AS "facet!",
    value AS "value!",
    key_id AS "key_id?",
    label AS "label?",
    user_login_name AS "user_login_name?"
FROM (
    SELECT
        facet,
        value,
        key_id,
        label,
        user_login_name,
        row_number() OVER (PARTITION BY facet ORDER BY value DESC) AS facet_rank
    FROM (
        SELECT
            CASE
                WHEN GROUPING(user_value) = 0 THEN 'user'
                WHEN GROUPING(model_value) = 0 AND model_value IS NOT NULL THEN 'model'
                WHEN GROUPING(target_value) = 0 AND target_value IS NOT NULL THEN 'target'
                WHEN GROUPING(date_value) = 0 THEN 'date'
                WHEN GROUPING(client_key_id) = 0 AND client_key_value IS NOT NULL THEN 'client_key'
                WHEN GROUPING(request_state) = 0 THEN 'state'
                WHEN GROUPING(redaction_value) = 0 THEN 'redaction'
            END AS facet,
            CASE
                WHEN GROUPING(user_value) = 0 THEN user_value
                WHEN GROUPING(model_value) = 0 AND model_value IS NOT NULL THEN model_value
                WHEN GROUPING(target_value) = 0 AND target_value IS NOT NULL THEN target_value
                WHEN GROUPING(date_value) = 0 THEN date_value
                WHEN GROUPING(client_key_id) = 0 AND client_key_value IS NOT NULL THEN client_key_value
                WHEN GROUPING(request_state) = 0 THEN request_state
                WHEN GROUPING(redaction_value) = 0 THEN redaction_value
            END AS value,
            CASE WHEN GROUPING(client_key_id) = 0 THEN client_key_id END AS key_id,
            CASE WHEN GROUPING(client_key_id) = 0 THEN client_key_value END AS label,
            CASE WHEN GROUPING(client_key_id) = 0 THEN user_login_name END AS user_login_name
        FROM bounded
        GROUP BY GROUPING SETS (
            (user_value),
            (model_value),
            (target_value),
            (date_value),
            (client_key_id, client_key_value, user_login_name),
            (request_state),
            (redaction_value)
        )
    ) grouped
    WHERE facet IS NOT NULL
) ranked
WHERE facet_rank <= $5
ORDER BY "facet!" ASC, "value!" DESC
