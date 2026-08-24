INSERT INTO request_record_raw_payloads(
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
SELECT
    $1,
    created_at,
    $2,
    $3,
    $4,
    $5
FROM request_records
WHERE event_id = $1
ON CONFLICT (created_at, event_id)
DO UPDATE SET
    raw_object_key = EXCLUDED.raw_object_key,
    raw_object_size_bytes = EXCLUDED.raw_object_size_bytes,
    raw_object_sha256 = EXCLUDED.raw_object_sha256,
    raw_object_expires_at = EXCLUDED.raw_object_expires_at
