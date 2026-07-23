INSERT INTO request_record_raw_payloads_overflow (
    event_id,
    created_at,
    request_raw_json,
    response_raw_body
)
SELECT
    event_id,
    created_at,
    request_raw_json,
    response_raw_body
FROM request_record_raw_payloads_default
WHERE created_at >= $1
  AND created_at < $2;
