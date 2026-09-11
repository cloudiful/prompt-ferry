SELECT COALESCE(SUM(total_tokens), 0)::BIGINT AS "total_tokens!"
FROM request_records
WHERE endpoint_id = $1
  AND request_category = 'ai'
  AND event_kind = 'request'
  AND created_at >= $2
