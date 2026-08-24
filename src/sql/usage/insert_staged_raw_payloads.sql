INSERT INTO request_record_raw_payloads (
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
SELECT
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
FROM request_record_raw_payloads_overflow;
