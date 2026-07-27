SELECT
    event_id,
    created_at,
    request_raw_json,
    response_raw_body
FROM request_record_raw_payloads
WHERE created_at >= $1
  AND created_at < $2
  AND raw_object_key IS NULL
  AND (request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL)
ORDER BY created_at ASC, event_id ASC
LIMIT $3
