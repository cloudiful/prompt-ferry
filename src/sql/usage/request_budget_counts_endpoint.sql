SELECT
    COUNT(*) FILTER (WHERE created_at >= $3)::BIGINT AS "daily_count?",
    COUNT(*) FILTER (WHERE created_at >= $4)::BIGINT AS "monthly_count?"
FROM request_records
WHERE event_kind = 'request'
  AND request_category = $1
  AND endpoint_id = $2
  AND NOT (
      request_state IN ('failed', 'aborted')
      AND error_code = 'budget_exceeded'
  )
