INSERT INTO request_record_raw_payloads_overflow (
    event_id,
    created_at,
    request_raw_json,
    response_raw_body,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
SELECT
    event_id,
    created_at,
    request_raw_json,
    response_raw_body,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
FROM request_record_raw_payloads_default
WHERE created_at >= $1
  AND created_at < $2;
